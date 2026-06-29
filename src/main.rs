mod app;
mod cli;
mod io;
mod models;
mod output;
mod parser;
mod plugins;

use anyhow::Result;
use io::config::apply_rc_defaults;
use io::git::apply_repo_top;

fn main() -> Result<()> {
    let mut cli = cli::parse_cli();
    apply_rc_defaults(&mut cli);
    apply_repo_top(&mut cli)?;
    app::run(cli)
}
