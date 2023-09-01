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

    DB.get().unwrap().init_table(ALL_PKGS).unwrap();
    DB.get().unwrap().init_table(INSTALLED_PKGS).unwrap();

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
