use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, BufReader};
use std::ops::Deref;
use std::path::Path;
use std::process::{Command, ExitStatus};

use flate2::bufread::GzDecoder;
use log::{info, trace, warn};
use sha2::{Digest, Sha512};
use tar::Archive;

use crate::db::Db;
use crate::pkg::{Installed, Local, Package};
use crate::pkgfile::PackageFile;
use crate::{DynResult, ALL_PKGS, CONFIG, DB, INSTALLED_PKGS};

#[cfg(not(feature = "parallel"))]
mod blocking;
#[cfg(not(feature = "parallel"))]
pub use blocking::*;
#[cfg(feature = "parallel")]
mod parallel;
#[cfg(feature = "parallel")]
pub use parallel::*;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PayloadPackage {
    file: PackageFile,
    manually_selected: bool,
    manually_added: bool,
}

impl Deref for PayloadPackage {
    type Target = PackageFile;

    fn deref(&self) -> &Self::Target {
        &self.file
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Payload {
    packages: HashSet<PayloadPackage>,
}

impl Payload {
    /// Check which packages have to be installed.
    fn check_install(&mut self) -> DynResult<()> {
        let db = DB.get().unwrap();
        let pkgs = db.get_iter(
            INSTALLED_PKGS,
            self.packages.iter().map(|x| x.info.name.as_str()),
        )?;

        self.packages.retain(|payload_pkg| {
            for db_pkg in pkgs.iter().flatten() {
                if db_pkg.info.version >= payload_pkg.info.version {
                    return false;
                }
            }
            true
        });

        Ok(())
    }

    /// Check the SHA512 checksum of a file at `path`.
    fn check_sha512(path: impl AsRef<Path>, sha512: &str) -> DynResult<bool> {
        info!("Checking SHA512 checksum.");

        let sha512 = hex::decode(sha512)?;
        trace!("Reference: {:x?}", sha512);

        let mut hasher = Sha512::new();

        let binary = fs::read(path)?;

        hasher.update(&binary);
        let result = hasher.finalize();

        trace!("Calculated: {:x?}", result);

        Ok(result[..] == sha512[..])
    }

    /// Check the SHA512 checksum of all `package` tarballs.
    fn check_sha512_pkgs(&self) -> DynResult<()> {
        let conf = CONFIG.get().unwrap();
        println!("Checking SHA512 checksums...");

        for pkg in &self.packages {
            let tar_name = format!("{}_{}.tar.gz", pkg.info.name, pkg.info.version);

            if let Some(checksum) = &pkg.source.checksum {
                assert!(
                    Self::check_sha512(&conf.sources_path().join(tar_name), checksum)?,
                    "Invalid checksum in package {}!",
                    pkg.info.name
                );
            }
        }

        Ok(())
    }

    /// Decompress a tarball.
    fn decompress_tarball(path: impl AsRef<Path>, destination: impl AsRef<Path>) -> io::Result<()> {
        info!("Decompressing tarball {}.", path.as_ref().to_string_lossy(),);

        let tar_gz = BufReader::new(File::open(path)?);
        let tar = GzDecoder::new(tar_gz);
        let mut archive = Archive::new(tar);
        archive.unpack(destination)?;

        Ok(())
    }

    /// Decompress all `package` tarballs.
    fn decompress_pkgs(&self) -> DynResult<()> {
        let conf = CONFIG.get().unwrap();
        println!("Decompressing packages...");

        for pkg in &self.packages {
            let tar_name = format!("{}_{}.tar.gz", pkg.info.name, pkg.info.version);
            let tar = conf.sources_path().join(tar_name);

            let untar = conf
                .builds_path()
                .join(format!("{}_{}", pkg.info.name, pkg.info.version));
            fs::create_dir_all(&untar)?;
            Self::decompress_tarball(&tar, &untar)?;
        }

        Ok(())
    }

    /// Run a command `cmd` with environment variables `env`.
    fn run_command<I, K, V>(cmd: &str, env: I) -> DynResult<ExitStatus>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        let output = Command::new("sh").arg("-c").arg(cmd).envs(env).output()?;

        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.is_empty() {
            warn!("Command stderr: {}", stderr);
        }
        trace!(
            "Command stdout: {}",
            String::from_utf8_lossy(&output.stdout)
        );

        Ok(output.status)
    }

    /// Build all `packages` using their build instructions.
    fn build_pkgs(&self) -> DynResult<()> {
        let conf = CONFIG.get().unwrap();
        println!("Building packages...");

        for pkg in &self.packages {
            let untar = conf
                .builds_path()
                .join(format!("{}_{}", pkg.info.name, pkg.info.version));
            let env = [("source", untar.as_path())];

            if let Some(cmd) = &pkg.source.build {
                println!("Building {}...", pkg.info.name);
                let status = Self::run_command(cmd, env)?;
                assert!(status.success(), "Build failed!");
            }
        }

        Ok(())
    }

    /// Install all `packages` using their install instructions.
    fn install_pkgs(&self) -> DynResult<()> {
        let conf = CONFIG.get().unwrap();
        println!("Installing packages...");

        for pkg in &self.packages {
            let untar = conf
                .builds_path()
                .join(format!("{}_{}", pkg.info.name, pkg.info.version));
            fs::create_dir_all(conf.binaries_path())?;
            let env = [
                ("source", untar.as_path()),
                ("binary", conf.binaries_path()),
            ];

            let status = Self::run_command(&pkg.source.install, env)?;
            assert!(status.success(), "Build failed!");
        }

        Ok(())
    }

    /// Write the payload to the database.
    fn write_db(&self) -> DynResult<()> {
        let db = DB.get().unwrap();
        for payload_pkg in &self.packages {
            let name = payload_pkg.info.name.as_str();
            let installed_new = match payload_pkg.manually_selected {
                true => Installed::Manually,
                false => Installed::Automatically,
            };
            let added = payload_pkg.manually_added;

            db.modify(INSTALLED_PKGS, name, |pkg| match pkg {
                Some(mut pkg) => {
                    let installed_old = pkg.local.installed;
                    pkg.local = Local {
                        installed: installed_old.update(installed_new),
                        added: payload_pkg.manually_added || added,
                    };
                    Some(pkg)
                }
                None => Some(Package::from_file(
                    payload_pkg.file.clone(),
                    Local {
                        installed: installed_new,
                        added: payload_pkg.manually_added,
                    },
                )),
            })?;

            if let Some(pkg) = db.get(INSTALLED_PKGS, name)? {
                db.set(ALL_PKGS, name, pkg)?;
            }
        }

        Ok(())
    }

    pub fn new() -> Self {
        Self {
            packages: HashSet::new(),
        }
    }

    /// Add a package and its dependencies to the payload.
    /// This marks the package as manually installed.
    pub fn add_pkg(&mut self, pkg: &str) -> DynResult<()> {
        let db = DB.get().unwrap();
        let pkg = db
            .get(ALL_PKGS, pkg)?
            .unwrap_or_else(|| panic!("Package {pkg} not found!"));

        if let Some(deps) = &pkg.info.dependencies {
            let pkgs = db.get_iter(
                ALL_PKGS,
                deps.iter().map(|x| x.as_str()).collect::<Vec<&str>>(),
            )?;

            for (key, pkg) in deps.iter().zip(pkgs.into_iter()) {
                let pkg = pkg.unwrap_or_else(|| panic!("Dependency {} not found!", key));
                self.packages.insert(PayloadPackage {
                    file: pkg.into(),
                    manually_selected: false,
                    manually_added: false,
                });
            }
        }

        self.packages.insert(PayloadPackage {
            file: pkg.into(),
            manually_selected: true,
            manually_added: false,
        });

        Ok(())
    }

    /// Add a package file and its dependencies to the payload.
    /// This marks the package as manually installed and added.
    pub fn add_pkgfile(&mut self, pkgfile: PackageFile) -> DynResult<()> {
        let db = DB.get().unwrap();

        if let Some(deps) = &pkgfile.info.dependencies {
            let pkgs = db.get_iter(
                ALL_PKGS,
                deps.iter().map(|x| x.as_str()).collect::<Vec<&str>>(),
            )?;

            for (key, pkg) in deps.iter().zip(pkgs.into_iter()) {
                let pkg = pkg.unwrap_or_else(|| panic!("Dependency {} not found!", key));
                self.packages.insert(PayloadPackage {
                    file: pkg.into(),
                    manually_selected: false,
                    manually_added: false,
                });
            }
        }

        self.packages.insert(PayloadPackage {
            file: pkgfile,
            manually_selected: true,
            manually_added: true,
        });

        Ok(())
    }
}
