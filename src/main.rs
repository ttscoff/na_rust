mod app;
mod cli;
mod io;
mod models;
mod output;
mod parser;
mod plugins;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    app::run(cli)
}
