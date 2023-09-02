use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, BufReader, Write};
use std::ops::Deref;
use std::path::Path;
use std::process::{Command, ExitStatus};

use flate2::bufread::GzDecoder;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use inquire::Confirm;
use log::{info, trace, warn};
use sha2::{Digest, Sha512};
use tar::Archive;

use crate::db::Db;
use crate::pkg::{Installed, Local, Package};
use crate::pkgfile::PackageFile;
use crate::{exit_with_message, DynResult, ALL_PKGS, CONFIG, DB, INSTALLED_PKGS};

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

#[derive(Clone, Debug)]
struct MultiProgressFormat<'a> {
    multiprogress: &'a MultiProgress,
    message: String,
    longest_message: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Payload {
    packages: HashSet<PayloadPackage>,
}

impl Payload {
    /// Download a tarball from a URL.
    async fn download_source<'a>(
        url: &str,
        path: impl AsRef<Path>,
        mpb: Option<MultiProgressFormat<'a>>,
    ) -> DynResult<()> {
        let response = reqwest::get(url).await?;
        let total_size = response.content_length().unwrap();

        let pb = mpb.map(|MultiProgressFormat { multiprogress: mpb, message, longest_message }| {
            let pb = mpb.add(ProgressBar::new(total_size));
            pb.set_style(
            ProgressStyle::default_bar()
                .template(&format!("{{spinner:.green}} {{msg:{longest_message}!}} [{{wide_bar:.cyan/blue}}] {{bytes}}/{{total_bytes}} ({{bytes_per_sec}}, {{eta}})")).unwrap()
                .progress_chars("#>-")
            );
            pb.set_message(message);
            pb
        });

        info!(
            "Downloading file {} from {}.",
            path.as_ref().to_string_lossy(),
            url
        );

        let mut file = fs::File::create(path)?;
        let mut downloaded: u64 = 0;
        let mut stream = response.bytes_stream();

        while let Some(item) = stream.next().await {
            let chunk = item?;
            file.write_all(&chunk)?;
            downloaded = (downloaded + (chunk.len() as u64)).min(total_size);
            if let Some(pb) = &pb {
                pb.set_position(downloaded);
            }
        }

        // pb.finish_with_message(&format!("Downloaded {} to {}", url, path));
        if let Some(pb) = &pb {
            pb.finish();
        }

        Ok(())
    }

    /// Download all `packages`.
    async fn download_pkgs(&self) -> DynResult<()> {
        let conf = CONFIG.get().unwrap();
        println!("Downloading packages...");
        let mpb = MultiProgress::new();

        let longest_message = self
            .packages
            .iter()
            .map(|pkg| pkg.info.name.len())
            .max()
            .unwrap();

        let futures = FuturesUnordered::new();
        for pkg in &self.packages {
            let tar_name = format!("{}_{}.tar.gz", pkg.info.name, pkg.info.version);
            let tar = conf.sources_path().join(tar_name);
            fs::create_dir_all(conf.sources_path())?;
            let future = Self::download_source(
                &pkg.source.url,
                tar,
                Some(MultiProgressFormat {
                    multiprogress: &mpb,
                    message: pkg.info.name.clone(),
                    longest_message,
                }),
            );
            futures.push(future);
        }

        let _: Vec<_> = futures.collect().await;
        Ok(())
    }

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
                    // TODO: Mark as manually installed
                    return false;
                }
            }
            true
        });
        if self.packages.is_empty() {
            exit_with_message(
                "All packages are already installed and up-to-date.",
                exitcode::OK,
            );
        }

        println!("Packages marked to be installed:");
        let mut iter = self.packages.iter();
        print!("{}", iter.next().expect("empty package list").info.name);
        for pkg in iter {
            print!(", {}", pkg.info.name)
        }
        println!();

        let ans = Confirm::new("Do you want to install these packages?")
            .with_default(false)
            .prompt()?;

        if !ans {
            exit_with_message("Aborting...", exitcode::OK);
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
                if !Self::check_sha512(&conf.sources_path().join(tar_name), checksum)? {
                    exit_with_message(
                        format!("Invalid checksum in package {}!", pkg.info.name),
                        exitcode::SOFTWARE, // TODO: Flag to ignore checksum
                    )
                }
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
        // TODO: Progressbar

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
            warn!("Command stderr: {stderr}");
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stdout.is_empty() {
            trace!("Command stdout: {stdout}");
        }

        Ok(output.status)
    }

    /// Build all `packages` using their build instructions.
    fn build_pkgs(&self) -> DynResult<()> {
        let conf = CONFIG.get().unwrap();
        println!("Building packages...");
        // TODO: Progressbar

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
        // TODO: Progressbar

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
                true => Installed::Manually(payload_pkg.file.info.version.clone()),
                false => Installed::Automatically(payload_pkg.file.info.version.clone()),
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
        let pkg = db.get(ALL_PKGS, pkg)?.unwrap_or_else(|| {
            exit_with_message(format!("Package {pkg} not found!"), exitcode::DATAERR)
        });

        if let Some(deps) = &pkg.info.dependencies {
            let pkgs = db.get_iter(
                ALL_PKGS,
                deps.iter().map(|x| x.as_str()).collect::<Vec<&str>>(),
            )?;

            for (key, pkg) in deps.iter().zip(pkgs.into_iter()) {
                let pkg = pkg.unwrap_or_else(|| {
                    exit_with_message(format!("Dependency {key} not found!"), exitcode::DATAERR)
                });
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
                let pkg = pkg.unwrap_or_else(|| {
                    exit_with_message(format!("Dependency {key} not found!"), exitcode::DATAERR)
                });
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
    pub async fn install(mut self) -> DynResult<()> {
        self.check_install()?;
        self.download_pkgs().await?;
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
    use std::str::FromStr;

    use redb::Database;
    use semver::Version;

    use super::*;
    use crate::config::{Config, ConfigDirs};
    use crate::db::Db;
    use crate::pkg::{Installed, Local, Package, PackageInfo, Source};
    use crate::{ALL_PKGS, DB, INSTALLED_PKGS};

    #[tokio::test]
    async fn test_download() {
        // init_logging();
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("topgrade.tar.gz");

        Payload::download_source("https://github.com/topgrade-rs/topgrade/releases/download/v12.0.2/topgrade-v12.0.2-x86_64-apple-darwin.tar.gz", &path, None).await.unwrap();

        assert!(path.exists());
    }

    #[tokio::test]
    async fn test_decompress_tarball() {
        // init_logging();
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path();

        Payload::download_source("https://github.com/topgrade-rs/topgrade/releases/download/v12.0.2/topgrade-v12.0.2-x86_64-apple-darwin.tar.gz", &path.join("topgrade.tar.gz"), None).await.unwrap();
        Payload::decompress_tarball(path.join("topgrade.tar.gz"), path).unwrap();

        assert!(path.join("topgrade").exists());
    }

    #[tokio::test]
    async fn test_check_sha512() {
        // init_logging();
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("topgrade.tar.gz");

        Payload::download_source("https://github.com/topgrade-rs/topgrade/releases/download/v12.0.2/topgrade-v12.0.2-x86_64-apple-darwin.tar.gz", &path, None).await.unwrap();
        assert!(
            Payload::check_sha512(
                path, "45dfddf13e8f5a5eb4a95dde6743f42f216ed6d3751d7430dae5f9e0dc54e67a400e6572789fb9984ff1c80bdee42a92112a76d5399436e857e723b653b366f1"
            ).unwrap()
        );
    }

    #[tokio::test]
    async fn test_payload() {
        // init_logging();
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
        let db = DB.get().unwrap();
        db.init_table(ALL_PKGS).unwrap();
        db.init_table(INSTALLED_PKGS).unwrap();

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

        db.set(ALL_PKGS, "topgrade", topgrade.clone()).unwrap();

        let mut payload = Payload::new();
        payload.add_pkg("topgrade").unwrap();
        payload.install().await.unwrap();

        let topgrade_table = db.get(ALL_PKGS, "topgrade").unwrap().unwrap();
        assert_eq!(
            topgrade_table.local.installed,
            Installed::Manually(Version::from_str("12.0.2").unwrap())
        );
        assert!(CONFIG
            .get()
            .unwrap()
            .binaries_path()
            .join("topgrade")
            .exists());
    }
}
