use std::fs;
use std::sync::OnceLock;
use std::{error::Error, process::exit};

use clap::Parser;
use cli::*;
use config::Config;
use db::{Db, DbPackage};
use directories::ProjectDirs;
use exitcode::ExitCode;
use log::{info, warn, LevelFilter};
use nucleo_matcher::pattern::{CaseMatching, Pattern};
use nucleo_matcher::Matcher;
use payload::Payload;
use pkg::Package;
use pkgfile::PackageFile;
use redb::{Database, ReadableTable, TableDefinition};
use simplelog::{ColorChoice, TermLogger, TerminalMode};

use crate::pkg::Installed;

mod cli;
mod config;
mod db;
mod payload;
mod pkg;
mod pkgfile;

static CONFIG: OnceLock<Config> = OnceLock::new();
static ALL_PKGS: TableDefinition<&str, DbPackage> = TableDefinition::new("all_pkgs");
static INSTALLED_PKGS: TableDefinition<&str, DbPackage> = TableDefinition::new("installed_pkgs");
static DB: OnceLock<Database> = OnceLock::new();
static DEBUG: OnceLock<bool> = OnceLock::new();

pub type DynResult<T> = Result<T, Box<dyn Error>>;

pub fn init_logging() {
    TermLogger::init(
        LevelFilter::Trace,
        simplelog::Config::default(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )
    .unwrap();
}

pub fn exit_with_message(message: impl AsRef<str>, exitcode: ExitCode) -> ! {
    let mut prepend = String::new();
    let mut append = String::new();
    if exitcode::is_error(exitcode) {
        prepend.push_str("\x1b[31mError!\x1b[0m ");
        append.push_str("\nAborting...");
    }
    println!("{prepend}{}{append}", message.as_ref());
    exit(exitcode);
}

#[cfg(feature = "parallel")]
#[tokio::main]
async fn main() -> DynResult<()> {
    color_eyre::install().unwrap();

    read_args().await;

    Ok(())
}

#[cfg(not(feature = "parallel"))]
fn main() {
    color_eyre::install().unwrap();

    read_args()
}

pub async fn read_args() {
    let cli = Cli::parse();

    #[cfg(debug_assertions)]
    DEBUG.set(cli.debug).expect("error setting debug flag");

    if *DEBUG.get_or_init(|| false) {
        init_logging();
    }

    let mut conf_path;
    match cli.config {
        Some(conf) => conf_path = conf,
        None => {
            conf_path = ProjectDirs::from("de", "mercurium", "mercurium")
                .unwrap() // TODO: Fallback
                .config_dir()
                .to_owned();
            conf_path.push("config.toml");
        }
    }

    CONFIG
        .set(Config::load(&conf_path).unwrap())
        .expect("error setting config");
    DB.set(
        Database::create(CONFIG.get().unwrap().packages_path().join("packages.db"))
            .unwrap_or_else(|_| exit_with_message("Couldn't create database", exitcode::CANTCREAT)),
    )
    .expect("error setting database");

    DB.get()
        .unwrap()
        .init_table(ALL_PKGS)
        .expect("error initiating database tables");
    DB.get()
        .unwrap()
        .init_table(INSTALLED_PKGS)
        .expect("error initiating database tables");

    match &cli.command {
        Commands::Install(args) => {
            if args.local {
                install_local(args).await;
            } else {
                install(args).await;
            }
        }
        Commands::Add(args) => add(args),
        Commands::Remove(args) => remove(args),
        Commands::Update(args) => update(args).await, // TODO
        Commands::Search(args) => search(args),
        Commands::List(args) => list(args),
        #[cfg(debug_assertions)]
        Commands::Config => config(),
    }
}

async fn install_local(args: &InstallArgs) {
    let InstallArgs { pkgs, .. } = args;

    let mut pkgfiles: Vec<PackageFile> = Vec::new();
    for pkg in pkgs {
        let pkg_content = fs::read_to_string(pkg)
            .unwrap_or_else(|_| exit_with_message("Couldn't access file", exitcode::NOINPUT));

        let pkgfile: PackageFile = toml::from_str(&pkg_content).unwrap_or_else(|_| {
            exit_with_message("Invalid package file format", exitcode::DATAERR)
        });
        pkgfiles.push(pkgfile);
    }

    let mut payload = Payload::new();
    for pkg in pkgfiles {
        payload.add_pkgfile(pkg).expect("error reading database");
    }
    payload.install().await.expect("error installing packages"); // TODO: Better errors
}

async fn install(args: &InstallArgs) {
    let InstallArgs { pkgs, .. } = args;

    let mut payload = Payload::new();
    for pkg in pkgs {
        payload.add_pkg(pkg).expect("error reading database");
    }
    payload.install().await.expect("error installing packages"); // TODO: Better errors
}

fn add(args: &AddArgs) {
    let AddArgs { pkgs } = args;

    for pkg in pkgs {
        let pkg_content = fs::read_to_string(pkg)
            .unwrap_or_else(|_| exit_with_message("Couldn't access file", exitcode::NOINPUT));
        let pkgfile: PackageFile = toml::from_str(&pkg_content).unwrap_or_else(|_| {
            exit_with_message("Invalid package file format", exitcode::DATAERR)
        });

        info!("Adding package {} to database.", pkgfile.info.name);
        pkgfile.add_to_db().expect("error modifying database");
    }
}

fn remove(args: &RemoveArgs) {
    let RemoveArgs { pkgs } = args;

    // TODO: Remove!
    let db = DB.get().unwrap();
    for pkg_name in pkgs {
        info!("Removing package {}.", pkg_name);
        db.modify(ALL_PKGS, pkg_name.as_str(), |val| {
            let mut val = val.unwrap();
            val.local.installed = Installed::False;
            Some(val)
        })
        .expect("error modifying database");
        db.remove(INSTALLED_PKGS, pkg_name.as_str())
            .expect("error modifying database");
    }
}

async fn update(args: &UpdateArgs) {
    let UpdateArgs { pkgs } = args;

    let db = DB.get().unwrap();
    let mut payload = Payload::new();

    match pkgs {
        Some(pkgs) => {
            let iter = db
                .get_iter(INSTALLED_PKGS, pkgs.iter().map(|k| k.as_str()))
                .expect("error reading database")
                .into_iter()
                .zip(pkgs)
                .map(|(pkg, name)| {
                    pkg.unwrap_or_else(|| {
                        exit_with_message(format!("Package {} not found!", name), exitcode::DATAERR)
                    })
                })
                .filter(|pkg| {
                    if let Some(installed_ver) = pkg.local.installed.version() {
                        &pkg.info.version > installed_ver
                    } else {
                        warn!("Invalid database state: Package {} in table INSTALLED_PKGS, but installed is set to False.", pkg.info.name);
                        false
                    }
                });

            for pkg in iter {
                payload
                    .add_pkg(&pkg.info.name) // Optimization: Take DbPackage directly
                    .expect("error reading database");
            }
        }
        None => {
            let read_txn = db.begin_read().expect("error reading database");
            let read_table = read_txn
                .open_table(INSTALLED_PKGS)
                .expect("error reading database");

            let iter = read_table
                .iter()
                .expect("error reading database")
                .map(|pkg| Into::<Package>::into(pkg.as_ref().expect("error reading database").1.value()))
                .filter(|pkg| {
                    if let Some(installed_ver) = pkg.local.installed.version() {
                        &pkg.info.version > installed_ver
                    } else {
                        warn!("Invalid database state: Package {} in table INSTALLED_PKGS, but installed is set to False.", pkg.info.name);
                        false
                    }
                });

            for pkg in iter {
                payload
                    .add_pkg(&pkg.info.name) // Optimization: Take DbPackage directly
                    .expect("error reading database");
            }
        }
    }

    payload.install().await.expect("error installing packages"); // TODO: Better errors
}

fn search(args: &SearchArgs) {
    let SearchArgs { pkg, installed } = args;

    let db = DB.get().unwrap();
    let read_txn = db.begin_read().expect("error reading database");
    let read_table = read_txn
        .open_table(ALL_PKGS)
        .expect("error reading database");

    let iter = read_table
        .iter()
        .expect("error reading database")
        .map(|x| x.expect("error reading database"))
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

fn list(args: &ListArgs) {
    let ListArgs { all } = args;

    let db = DB.get().unwrap();
    let read_txn = db.begin_read().expect("error reading database");
    let read_table = if *all {
        read_txn
            .open_table(ALL_PKGS)
            .expect("error reading database")
    } else {
        read_txn
            .open_table(INSTALLED_PKGS)
            .expect("error reading database")
    };

    let mut pkgs: Vec<(String, bool)> = Vec::new();

    for pkg in read_table.iter().expect("error reading database") {
        let (key, value) = pkg.expect("error reading database");

        pkgs.push((key.value().to_owned(), value.value().installed.into()));
    }

    pkgs.sort_by_key(|x| x.0.to_lowercase());
    pkgs.into_iter().for_each(|(name, installed)| {
        let mut to_print = name;
        if *all && installed {
            to_print.push_str(" [Installed]");
        }
        println!("{to_print}");
    });
}

#[cfg(debug_assertions)]
fn config() {
    dbg!(CONFIG.get().unwrap());
}
