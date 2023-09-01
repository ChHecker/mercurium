use std::path::{Path, PathBuf};
use std::{fs, io};

use directories::{BaseDirs, ProjectDirs};
use log::{error, info};
use serde::Deserialize;

/// The configuration.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Config {
    /// The different directories to act on.
    pub directories: ConfigDirs,
}

impl Config {
    /// Load the config from the `path`.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, io::Error> {
        info!("Loading config from {}.", path.as_ref().to_string_lossy());

        let out = if path.as_ref().exists() {
            let conf_str = fs::read_to_string(path)?;
            match toml::from_str(&conf_str) {
                Ok(conf) => conf,
                Err(_) => {
                    error!("Invalid config! Using default configuration.");
                    Config::default()
                }
            }
        } else {
            info!("Config file not found! Using default configuration.");
            Config::default()
        };

        fs::create_dir_all(out.sources_path())?;
        fs::create_dir_all(out.builds_path())?;
        fs::create_dir_all(out.binaries_path())?;
        fs::create_dir_all(out.packages_path())?;

        Ok(out)
    }

    /// Path to download source files to.
    pub fn sources_path(&self) -> &Path {
        &self.directories.sources
    }

    /// Path the source files should be decompressed to. It is also used to build the package.
    pub fn builds_path(&self) -> &Path {
        &self.directories.builds
    }

    /// Path to install the binaries to.
    pub fn binaries_path(&self) -> &Path {
        &self.directories.binaries
    }

    /// Path with the package database.
    pub fn packages_path(&self) -> &Path {
        &self.directories.packages
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct ConfigDirs {
    #[serde(default = "default_sources")]
    pub sources: PathBuf,
    #[serde(default = "default_builds")]
    pub builds: PathBuf,
    #[serde(default = "default_binaries")]
    pub binaries: PathBuf,
    #[serde(default = "default_packages")]
    pub packages: PathBuf,
}

impl Default for ConfigDirs {
    fn default() -> Self {
        Self {
            sources: default_sources(),
            builds: default_builds(),
            binaries: default_binaries(),
            packages: default_packages(),
        }
    }
}

fn default_sources() -> PathBuf {
    let dir = ProjectDirs::from("de", "mercurium", "mercurium")
        .unwrap()
        .cache_dir()
        .to_owned()
        .join("sources");
    dir
}

fn default_builds() -> PathBuf {
    let dir = ProjectDirs::from("de", "mercurium", "mercurium")
        .unwrap()
        .cache_dir()
        .to_owned()
        .join("builds");
    dir
}

fn default_binaries() -> PathBuf {
    let dir = BaseDirs::new()
        .unwrap()
        .home_dir()
        .to_owned()
        .join(".local")
        .join("bin");
    dir
}

fn default_packages() -> PathBuf {
    let dir = ProjectDirs::from("de", "mercurium", "mercurium")
        .unwrap()
        .data_dir()
        .to_owned();
    dir
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init_logging;

    #[test]
    fn load_config() {
        init_logging();

        let conf = "
            [directories]
            sources = \"/Users/chrobin/Documents/rust/mercurium/tests/sources\"
            builds = \"/Users/chrobin/Documents/rust/mercurium/tests/builds\"
            binaries = \"/Users/chrobin/Documents/rust/mercurium/tests/binaries\"
            packages = \"/Users/chrobin/Documents/rust/mercurium/tests\"
        ";

        let conf: Config = toml::from_str(conf).unwrap();
        dbg!(conf);
    }
}
