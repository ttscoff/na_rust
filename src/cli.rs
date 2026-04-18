use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "na",
    version,
    about = "Rust rewrite of na TaskPaper CLI",
    long_about = None
)]
pub struct Cli {
    /// Override file extension when searching for TaskPaper files.
    #[arg(short, long, default_value = "taskpaper")]
    pub extension: String,

    /// Work against a single global file instead of cwd scanning.
    #[arg(short = 'g', long)]
    pub global_file: Option<PathBuf>,

    /// Disable colorized output.
    #[arg(long, default_value_t = false)]
    pub no_color: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Show the next available actions.
    #[command(visible_alias = "show")]
    Next(NextArgs),
    /// Find actions by search terms.
    #[command(visible_aliases = ["grep", "search"])]
    Find(FindArgs),
    /// Add a new action.
    Add(AddArgs),
    /// Update existing actions.
    Update(UpdateArgs),
    /// Mark actions complete.
    Complete(UpdateArgs),
    /// Inspect or run plugins.
    Plugin(PluginArgs),
}

#[derive(Debug, Args)]
pub struct NextArgs {
    /// Optional TaskPaper-style filter expression.
    #[arg(value_name = "FILTER")]
    pub filter: Option<String>,

    /// Keep only one action per project.
    #[arg(long = "first-available", visible_alias = "available", default_value_t = false)]
    pub first_available: bool,

    /// Display matches from a specific TaskPaper file.
    #[arg(long)]
    pub file: Option<PathBuf>,

    /// Display matches from all known todo files.
    #[arg(long, default_value_t = false)]
    pub all: bool,

    /// Include hidden directories while traversing.
    #[arg(long, default_value_t = false)]
    pub hidden: bool,

    /// Recurse to depth when searching for files.
    #[arg(short = 'd', long)]
    pub depth: Option<usize>,

    /// Display matches from known todo files in history.
    #[arg(long = "in", visible_alias = "todo", value_name = "TODO", num_args = 0..)]
    pub in_todo: Vec<String>,
}

#[derive(Debug, Args)]
pub struct FindArgs {
    /// Search query terms or @search(...) expression.
    #[arg(value_name = "QUERY")]
    pub query: String,
}

#[derive(Debug, Args)]
pub struct AddArgs {
    /// Action text to append.
    #[arg(value_name = "TEXT")]
    pub text: String,
}

#[derive(Debug, Args, Clone)]
pub struct UpdateArgs {
    /// Search query for selecting actions.
    #[arg(value_name = "QUERY")]
    pub query: String,

    /// Tags to add (e.g. @today @home).
    #[arg(short, long, value_name = "TAG", num_args = 0..)]
    pub tag: Vec<String>,

    /// Tags to remove (e.g. @today @home).
    #[arg(long, value_name = "TAG", num_args = 0..)]
    pub untag: Vec<String>,

    /// Mark action as done.
    #[arg(long, default_value_t = false)]
    pub done: bool,
}

#[derive(Debug, Args)]
pub struct PluginArgs {
    #[command(subcommand)]
    pub command: PluginCommands,
}

#[derive(Debug, Subcommand)]
pub enum PluginCommands {
    /// List discovered plugins.
    List,
    /// Run plugin against selected actions.
    Run {
        #[arg(value_name = "PLUGIN")]
        plugin: String,
        #[arg(value_name = "QUERY")]
        query: String,
    },
}

#[cfg(test)]
mod tests {
    use super::{Cli, Commands};
    use clap::Parser;
    use std::path::PathBuf;

    #[test]
    fn next_available_alias_sets_first_available() {
        let cli = Cli::parse_from(["na", "next", "--available"]);
        match cli.command {
            Commands::Next(args) => assert!(args.first_available),
            _ => panic!("expected next command"),
        }
    }

    #[test]
    fn next_file_and_depth_flags_parse() {
        let cli = Cli::parse_from(["na", "next", "--file", "x.taskpaper", "--depth", "3"]);
        match cli.command {
            Commands::Next(args) => {
                assert_eq!(args.file, Some(PathBuf::from("x.taskpaper")));
                assert_eq!(args.depth, Some(3));
            }
            _ => panic!("expected next command"),
        }
    }

    #[test]
    fn find_command_alias_search_parses() {
        let cli = Cli::parse_from(["na", "search", "@home"]);
        match cli.command {
            Commands::Find(args) => assert_eq!(args.query, "@home"),
            _ => panic!("expected find command alias"),
        }
    }
}
