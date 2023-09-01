use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::pkg::{Installed, Local, Package, PackageInfo, Source};
use crate::{DynResult, ALL_PKGS, DB};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct PackageFile {
    #[serde(rename = "package")]
    pub info: PackageInfo,
    pub source: Source,
}

impl PackageFile {
    /// Adds the package file to the database.
    ///
    /// The package is marked as `added`. If it is not already in the database, it is also markes as not installed.
    pub fn add_to_db(self) -> DynResult<()> {
        let db = DB.get().unwrap();
        let name = self.info.name.clone();

        db.modify(ALL_PKGS, name.as_str(), |pkg| {
            let local = match pkg {
                Some(pkg) => {
                    let mut local = pkg.local;
                    local.added = true;
                    local
                }
                None => Local {
                    installed: Installed::False,
                    added: true,
                },
            };

            Some(Package::from_file(self, local))
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::str::FromStr;

    use semver::Version;

    use super::*;

    #[test]
    fn parse_toml() {
        // init_logging();

        let package_file: PackageFile = toml::from_str(
            "
                [package]
                name = \"topgrade\"
                authors = [\"topgrade-rs\"]
                license = \"GPL-3.0\"
                description = \"Upgrade all the things \"
                version = \"12.0.2\"
                repository = \"https://github.com/topgrade-rs/topgrade\"

                [source]
                url = \"https://github.com/topgrade-rs/topgrade/releases/download/v12.0.2/topgrade-v12.0.2-x86_64-apple-darwin.tar.gz\"
                install = \" mv ${source}/topgrade ${binary} \"
            "
        )
        .unwrap();
        dbg!(package_file);
    }

    #[test]
    fn parse_file() {
        // init_logging();

        let file: PackageFile =
            toml::from_str(&fs::read_to_string("packages/topgrade.pkg").unwrap()).unwrap();

        let local = PackageFile {
                        info: PackageInfo {
                            name: "topgrade".to_owned(),
                            version: Version::from_str("12.0.2").unwrap(),
                            license: "GPL3.0".to_owned(),
                            repository: Some("https://github.com/topgrade-rs/topgrade".to_owned()),
                            authors: Some(vec!["topgrade-rs".to_owned()]),
                            description: Some("Upgrade all the things".to_owned()),
                            dependencies: None,
                            build_dependencies: None,
                            provides: None,
                        },
                        source: Source {
                            url: "https://github.com/topgrade-rs/topgrade/releases/download/v12.0.2/topgrade-v12.0.2-x86_64-apple-darwin.tar.gz".to_owned(),
                            checksum: Some("45dfddf13e8f5a5eb4a95dde6743f42f216ed6d3751d7430dae5f9e0dc54e67a400e6572789fb9984ff1c80bdee42a92112a76d5399436e857e723b653b366f1".to_owned()),
                            build: None,
                            install: "mv ${source}/topgrade ${binary}".to_owned(),
                        },
                    };

        assert_eq!(file, local);
    }
}
