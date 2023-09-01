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

    /// Download a tarball from a URL.
    fn download_source(url: &str, path: impl AsRef<Path>) -> DynResult<()> {
        let response = reqwest::blocking::get(url)?;

        info!(
            "Downloading file {} from {}.",
            path.as_ref().to_string_lossy(),
            url
        );

        let content = response.bytes()?;
        fs::write(path, content)?;
        Ok(())
    }

    /// Download all `packages`.
    fn download_pkgs(&self) -> DynResult<()> {
        let conf = CONFIG.get().unwrap();
        println!("Downloading packages...");

        for pkg in &self.packages {
            let tar_name = format!("{}_{}.tar.gz", pkg.info.name, pkg.info.version);
            let tar = conf.sources_path().join(tar_name);
            fs::create_dir_all(conf.sources_path())?;
            Self::download_source(&pkg.source.url, &tar)?;
        }

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

            println!("Decompressing files...");
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

    /// Execute the payload.
    pub fn install(mut self) -> DynResult<()> {
        self.check_install()?;
        self.download_pkgs()?;
        self.check_sha512_pkgs()?;
        self.decompress_pkgs()?;
        self.build_pkgs()?;
        self.install_pkgs()?;
        self.write_db()?;
        println!("Done!");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::str::FromStr;

    use redb::{Database, ReadableTable};
    use semver::Version;

    use super::*;
    use crate::config::{Config, ConfigDirs};
    use crate::db::DbPackage;
    use crate::init_logging;
    use crate::pkg::{Local, Package, PackageInfo, Source};

    #[test]
    fn test_download() {
        init_logging();

        let path = PathBuf::from("tests/topgrade.tar.gz");

        Payload::download_source("https://github.com/topgrade-rs/topgrade/releases/download/v12.0.2/topgrade-v12.0.2-x86_64-apple-darwin.tar.gz", &path).unwrap();

        assert!(path.exists());
    }

    #[test]
    fn test_decompress_tarball() {
        init_logging();

        Payload::decompress_tarball("tests/topgrade.tar.gz", "tests/").unwrap();

        assert!(PathBuf::from("tests/topgrade").exists());
    }

    #[test]
    fn test_check_sha512() {
        init_logging();

        assert!(
            Payload::check_sha512(
                "tests/topgrade", "bf2a5d74f655456c0d725813bc2dc74fbca8d1afa5b23700f976deb029125299ccad2d775bbf15928cee948ac819235d844f92c0f265a17fc6c1314e2f6cdea4"
            ).unwrap()
        );
    }

    #[test]
    fn test_payload() {
        init_logging();
        let tmpdir = tempfile::tempdir().unwrap();

        CONFIG
            .set(Config {
                directories: ConfigDirs {
                    sources: tmpdir.path().join("sources"),
                    builds: tmpdir.path().join("builds"),
                    binaries: tmpdir.path().join("binaries"),
                    packages: tmpdir.path().to_owned(),
                },
            })
            .unwrap();
        let db_path = CONFIG
            .get()
            .expect("error getting config")
            .packages_path()
            .join("packages.db");
        DB.set(Database::create(db_path).expect("error creating database"))
            .expect("error setting database");
        let topgrade = Package {
                        info: PackageInfo {
                            name: "topgrade".to_owned(),
                            version: Version::from_str("12.0.2").unwrap(),
                            license: "GPL3.0".to_owned(),
                            repository: Some("https://github.com/topgrade-rs/topgrade".to_owned()),
                            authors: None,
                            description: None,
                            dependencies: None,
                            build_dependencies: None,
                            provides: None,
                        },
                        source: Source {
                            url: "https://github.com/topgrade-rs/topgrade/releases/download/v12.0.2/topgrade-v12.0.2-x86_64-apple-darwin.tar.gz".to_owned(),
                            checksum: None,
                            build: None,
                            install: "mv ${source}/topgrade ${binary}".to_owned(),
                        },
                        local: Local { installed: Installed::False, added: false}
                    };

        let db = DB.get().unwrap();
        let write_txn = db.begin_write().unwrap();
        {
            let mut write_table = write_txn.open_table(ALL_PKGS).unwrap();
            write_table
                .insert("topgrade", Into::<DbPackage>::into(topgrade))
                .unwrap();
        }
        write_txn.commit().unwrap();

        let mut payload = Payload::new();
        payload.add_pkg("topgrade").unwrap();
        payload.install().unwrap();

        let read_txn = db.begin_read().unwrap();
        let read_table = read_txn.open_table(ALL_PKGS).unwrap();
        assert_eq!(
            read_table
                .get("topgrade")
                .unwrap()
                .unwrap()
                .value()
                .installed,
            Installed::Manually
        );
        assert!(read_table.get("topgrade").unwrap().unwrap().value().added,);
        assert!(CONFIG
            .get()
            .unwrap()
            .binaries_path()
            .join("topgrade")
            .exists());
    }
}
