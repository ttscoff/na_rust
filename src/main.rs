mod app;
mod cli;
mod io;
mod models;
mod output;
mod parser;
mod plugins;

use anyhow::Result;
use clap::Parser;

use io::config::apply_rc_defaults;

fn main() -> Result<()> {
    let mut cli = cli::Cli::parse();
    apply_rc_defaults(&mut cli);
    app::run(cli)
}
