use std::fs;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};
use nucleo_matcher::pattern::{CaseMatching, Pattern};
use nucleo_matcher::Matcher;
use redb::ReadableTable;

use crate::db::Db;
use crate::pkg::Installed;
use crate::pkgfile::PackageFile;
use crate::{ALL_PKGS, DB, INSTALLED_PKGS};

#[cfg(not(feature = "parallel"))]
mod blocking;
#[cfg(not(feature = "parallel"))]
pub use blocking::*;
#[cfg(feature = "parallel")]
mod parallel;
#[cfg(feature = "parallel")]
pub use parallel::*;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Set a custom config file
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Install a package
    Install(InstallArgs),
    /// Add a package to the database
    Add(AddArgs),
    /// Remove a package
    Remove(RemoveArgs),
    /// Update all packages
    Update,
    /// Search for a package
    Search(SearchArgs),
    /// List installed packages
    List(ListArgs),
}

#[derive(Args)]
struct InstallArgs {
    /// Name of the pkgs
    pkg: Vec<String>,
    /// Use local pkgfiles
    #[arg(short, long)]
    local: bool,
}

#[derive(Args)]
struct AddArgs {
    /// Path of the pkgfiles.
    pkg: Vec<PathBuf>,
}

#[derive(Args)]
struct RemoveArgs {
    /// Name of the packages
    pkg: Vec<String>,
}

#[derive(Args)]
struct SearchArgs {
    /// Name of the package
    pkg: String,
    /// Only search installed packages
    #[arg(short, long)]
    installed: bool,
}

#[derive(Args)]
struct ListArgs {
    /// List all packages (whether installed or not)
    #[arg(short, long)]
    all: bool,
}

fn add(pkgs: &[impl AsRef<Path>]) {
    for pkg in pkgs {
        let pkg_content = fs::read_to_string(pkg).expect("error reading pkg file");
        let pkgfile: PackageFile = toml::from_str(&pkg_content).expect("invalid pkg file");
        pkgfile.add_to_db().unwrap();
    }
}

fn remove(pkgs: &[String]) {
    // TODO: Remove!
    let db = DB.get().unwrap();
    for pkg_name in pkgs {
        db.modify(ALL_PKGS, pkg_name.as_str(), |val| {
            let mut val = val.unwrap();
            val.local.installed = Installed::False;
            Some(val)
        })
        .unwrap();
        db.remove(INSTALLED_PKGS, pkg_name.as_str()).unwrap();
    }
}

fn search(pkg: &str, installed: bool) {
    let db = DB.get().unwrap();
    let read_txn = db.begin_read().unwrap();
    let read_table = read_txn.open_table(ALL_PKGS).unwrap();

    let iter = read_table
        .iter()
        .unwrap()
        .map(|x| x.unwrap())
        .filter(|x| x.1.value().installed.into() || !installed)
        .map(|x| x.0.value().to_owned().clone());

    let mut conf = nucleo_matcher::Config::DEFAULT;
    conf.ignore_case = true;
    let mut matcher = Matcher::new(conf);
    let mut matches: Vec<(String, u32)> =
        Pattern::parse(pkg, CaseMatching::Ignore).match_list(iter, &mut matcher);
    matches.sort_by_key(|(_, k)| *k);

    for (s, _) in matches {
        println!("{s}");
    }
}

fn list(all: bool) {
    let db = DB.get().unwrap();
    let read_txn = db.begin_read().unwrap();
    let read_table = if all {
        read_txn.open_table(ALL_PKGS).unwrap()
    } else {
        read_txn.open_table(INSTALLED_PKGS).unwrap()
    };

    let mut pkgs: Vec<(String, bool)> = Vec::new();

    for pkg in read_table.iter().unwrap() {
        let (key, value) = pkg.unwrap();

        pkgs.push((key.value().to_owned(), value.value().installed.into()));
    }

    pkgs.sort_by_key(|x| x.0.to_lowercase());
    pkgs.into_iter().for_each(|(name, installed)| {
        let mut to_print = name;
        if all && installed {
            to_print.push_str(" [Installed]");
        }
        println!("{to_print}");
    });
}
