use semver::Version;
use serde::{Deserialize, Serialize};

use crate::pkgfile::PackageFile;

/// A package.
#[derive(Clone, Debug, PartialEq)]
pub struct Package {
    /// General info on the package.
    pub info: PackageInfo,
    /// Info on the source and how to build and install the package.
    pub source: Source,
    /// Info on the local installation of the package.
    pub local: Local,
}

impl Package {
    /// Load a package from a package file.
    pub fn from_file(file: PackageFile, local: Local) -> Self {
        Self {
            info: file.info,
            source: file.source,
            local,
        }
    }
}

impl From<Package> for PackageFile {
    fn from(value: Package) -> Self {
        PackageFile {
            info: value.info,
            source: value.source,
        }
    }
}

/// General info of a package.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct PackageInfo {
    pub name: String,
    pub version: Version,
    pub license: String,
    pub repository: Option<String>,
    pub authors: Option<Vec<String>>,
    pub description: Option<String>,
    pub dependencies: Option<Vec<String>>,
    pub build_dependencies: Option<Vec<String>>,
    pub provides: Option<String>,
}

/// General info of a package.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct Source {
    pub url: String,
    pub checksum: Option<String>,
    pub build: Option<String>,
    pub install: String,
}

/// Info on the local installation of the package.
#[derive(Clone, Debug, PartialEq)]
pub struct Local {
    /// Whether a package is installed and if it's the case, whether manually or automatically.
    pub installed: Installed,
    /// Whether a package was manually added from a package file.
    pub added: bool,
}

/// Whether a package is installed and if it's the case, whether manually or automatically.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub enum Installed {
    Automatically(Version),
    Manually(Version),
    False,
}

impl Installed {
    /// Update the `Installed` value with a `new` value.
    /// If a package was automatically installed or not installed at all, take the new state.
    /// If it was manually installed and gets uninstalled, mark it as the latter.
    /// Otherwise, it stays manually installed.
    pub fn update(self, new: Installed) -> Installed {
        match self {
            Installed::Automatically(_) | Installed::False => new,
            Installed::Manually(_) => match new {
                Installed::Automatically(ver) | Installed::Manually(ver) => {
                    Installed::Manually(ver)
                }
                Installed::False => Installed::False,
            },
        }
    }

    /// Get the version of the package if installed.
    pub fn version(&self) -> Option<&Version> {
        match &self {
            Installed::Automatically(ver) | Installed::Manually(ver) => Some(ver),
            Installed::False => None,
        }
    }
}

impl From<Installed> for bool {
    fn from(value: Installed) -> Self {
        match value {
            Installed::Automatically(_) | Installed::Manually(_) => true,
            Installed::False => false,
        }
    }
}
