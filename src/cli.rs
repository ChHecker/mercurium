use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Set a custom config file
    #[arg(short, long)]
    pub config: Option<PathBuf>,
    /// Enable debugs
    #[cfg(debug_assertions)]
    #[arg(short, long)]
    pub debug: bool,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Install a package
    Install(InstallArgs),
    /// Add a package to the database
    Add(AddArgs),
    /// Remove a package
    Remove(RemoveArgs),
    /// Update all packages
    Update(UpdateArgs),
    /// Search for a package
    Search(SearchArgs),
    /// List installed packages
    List(ListArgs),
    #[cfg(debug_assertions)]
    Config,
}

#[derive(Args)]
pub struct InstallArgs {
    /// Name of the pkgs
    pub pkgs: Vec<String>,
    /// Use local pkgfiles
    #[arg(short, long)]
    pub local: bool,
}

#[derive(Args)]
pub struct AddArgs {
    /// Path of the pkgfiles.
    pub pkgs: Vec<PathBuf>,
}

#[derive(Args)]
pub struct RemoveArgs {
    /// Name of the packages
    pub pkgs: Vec<String>,
}
#[derive(Args)]
pub struct UpdateArgs {
    /// Name of the packages
    pub pkgs: Option<Vec<String>>,
}

#[derive(Args)]
pub struct SearchArgs {
    /// Name of the package
    pub pkg: String,
    /// Only search installed packages
    #[arg(short, long)]
    pub installed: bool,
}

#[derive(Args)]
pub struct ListArgs {
    /// List all packages (whether installed or not)
    #[arg(short, long)]
    pub all: bool,
}
