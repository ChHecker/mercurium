use clap::{CommandFactory, ValueEnum};
use clap_complete::{generate_to, Shell};
use std::cell::OnceCell;
use std::env;
use std::io::Error;

include!("src/cli.rs");

fn main() -> Result<(), Error> {
    let outdir = match env::var_os("OUT_DIR") {
        None => return Ok(()),
        Some(outdir) => outdir,
    };

    let mut cmd = Cli::command();

    let dir: OnceCell<PathBuf> = OnceCell::new();
    for &shell in Shell::value_variants() {
        let path = generate_to(shell, &mut cmd, "mercurium", &outdir)?;
        dir.set(path.parent().unwrap().to_owned()).ok();
    }

    println!(
        "cargo:warning=completion file is generated: {:?}",
        dir.get().unwrap()
    );

    Ok(())
}
