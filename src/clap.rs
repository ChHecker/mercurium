use std::fs;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};
use directories::ProjectDirs;
use nucleo_matcher::pattern::{CaseMatching, Pattern};
use nucleo_matcher::Matcher;
use redb::{Database, ReadableTable};

use crate::config::Config;
use crate::db::Db;
use crate::payload::Payload;
use crate::pkg::Installed;
use crate::pkgfile::PackageFile;
use crate::{ALL_PKGS, CONFIG, DB, INSTALLED_PKGS};

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

pub fn read_args() {
    let cli = Cli::parse();

    let mut conf_path;
    match cli.config {
        Some(conf) => conf_path = conf,
        None => {
            conf_path = ProjectDirs::from("de", "mercurium", "mercurium")
                .unwrap()
                .config_dir()
                .to_owned();
            conf_path.push("config.toml");
        }
    }

    CONFIG
        .set(Config::load(&conf_path).unwrap())
        .expect("error setting config");
    DB.set(
        Database::create(
            CONFIG
                .get()
                .expect("error getting config")
                .packages_path()
                .join("packages.db"),
        )
        .expect("error creating database"),
    )
    .expect("error creating database");

    match &cli.command {
        Commands::Install(args) => {
            if args.local {
                install_local(&args.pkg);
            } else {
                install(&args.pkg);
            }
        }
        Commands::Add(args) => add(&args.pkg),
        Commands::Remove(args) => remove(&args.pkg),
        Commands::Update => todo!(), // TODO
        Commands::Search(args) => search(&args.pkg, args.installed),
        Commands::List(args) => list(args.all),
    }
}

fn install_local(pkgs: &[impl AsRef<Path>]) {
    let mut pkgfiles: Vec<PackageFile> = Vec::new();
    for pkg in pkgs {
        let pkg_content = fs::read_to_string(pkg).expect("error reading pkg file");
        let pkgfile: PackageFile = toml::from_str(&pkg_content).expect("invalid pkg file");
        pkgfiles.push(pkgfile);
    }

    let mut payload = Payload::new();
    for pkg in pkgfiles {
        payload.add_pkgfile(pkg).unwrap();
    }
    payload.install().unwrap();
}

fn install(pkgs: &[String]) {
    let mut payload = Payload::new();
    for pkg in pkgs {
        payload.add_pkg(pkg).unwrap();
    }
    payload.install().unwrap();
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
        db.modify(INSTALLED_PKGS, pkg_name.as_str(), |val| {
            let mut val = val.expect("package not installed");
            val.local.installed = Installed::False;
            Some(val)
        })
        .unwrap();
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

    for pkg in read_table.iter().unwrap() {
        println!("{}", pkg.unwrap().0.value());
    }
}
