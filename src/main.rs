use std::error::Error;
use std::sync::OnceLock;

use clap::read_args;
use config::Config;
use db::DbPackage;
use redb::{Database, TableDefinition};

mod clap;
mod config;
mod db;
mod payload;
mod pkg;
mod pkgfile;

static CONFIG: OnceLock<Config> = OnceLock::new();
static ALL_PKGS: TableDefinition<&str, DbPackage> = TableDefinition::new("all_pkgs");
static INSTALLED_PKGS: TableDefinition<&str, DbPackage> = TableDefinition::new("installed_pkgs");
static DB: OnceLock<Database> = OnceLock::new();

pub type DynResult<T> = Result<T, Box<dyn Error>>;

#[test]
pub fn init_logging() {
    use log::LevelFilter;
    use simplelog::{ColorChoice, Config, TermLogger, TerminalMode};

    TermLogger::init(
        LevelFilter::Trace,
        Config::default(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )
    .unwrap();
    color_eyre::install().unwrap();
}

fn main() {
    read_args();
}
