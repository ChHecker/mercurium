use crate::payload::Payload;
use crate::{DynResult, CONFIG};
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use log::info;
use std::fs;
use std::io::Write;
use std::path::Path;

impl Payload {
    /// Download a tarball from a URL.
    async fn download_source(
        url: &str,
        path: impl AsRef<Path>,
        mpb: Option<&MultiProgress>,
    ) -> DynResult<()> {
        let response = reqwest::get(url).await?;
        let total_size = response.content_length().unwrap();

        let pb = mpb.map(|mpb| {
            let pb = mpb.add(ProgressBar::new(total_size));
            pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} {msg} [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})").unwrap()
                .progress_chars("#>-")
            );
            pb.set_message(format!("{}", path.as_ref().to_string_lossy()));
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

        let futures = FuturesUnordered::new();
        for pkg in &self.packages {
            let tar_name = format!("{}_{}.tar.gz", pkg.info.name, pkg.info.version);
            let tar = conf.sources_path().join(tar_name);
            fs::create_dir_all(conf.sources_path())?;
            let future = Self::download_source(&pkg.source.url, tar, Some(&mpb));
            futures.push(future);
        }

        let _: Vec<_> = futures.collect().await;
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
                        local: Local { installed: Installed::Manually, added: false}
                    };

        db.set(ALL_PKGS, "topgrade", topgrade.clone()).unwrap();

        let mut payload = Payload::new();
        payload.add_pkg("topgrade").unwrap();
        payload.install().await.unwrap();

        let topgrade_table = db.get(ALL_PKGS, "topgrade").unwrap().unwrap();
        assert_eq!(topgrade_table.local.installed, Installed::Manually);
        assert!(CONFIG
            .get()
            .unwrap()
            .binaries_path()
            .join("topgrade")
            .exists());
    }
}
