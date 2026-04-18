use crate::cli::{Cli, Commands, NextArgs, PluginCommands, UpdateArgs};
use crate::io::fs::{discover_taskpaper_files, discover_taskpaper_files_with_options};
use crate::models::action::Action;
use crate::models::todo::TodoFile;
use crate::output::formatter::{format_action, OutputStyle};
use crate::parser::search::{evaluate_query, Query};
use crate::plugins::registry::PluginRegistry;
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

pub fn run(cli: Cli) -> Result<()> {
    match &cli.command {
        Commands::Next(args) => run_next(&cli, args),
        Commands::Find(args) => run_find(&cli, &args.query),
        Commands::Add(args) => run_add(&cli, &args.text),
        Commands::Update(args) => run_update(&cli, args),
        Commands::Complete(args) => {
            let mut complete = args.clone();
            complete.done = true;
            run_update(&cli, &complete)
        }
        Commands::Plugin(args) => run_plugin(&cli, &args.command),
    }
}

fn load_todo_files(cli: &Cli) -> Result<Vec<TodoFile>> {
    let files = if let Some(path) = &cli.global_file {
        vec![path.clone()]
    } else {
        discover_taskpaper_files(&cli.extension)?
    };

    files
        .iter()
        .map(|path| TodoFile::load(path).with_context(|| format!("Failed to read {:?}", path)))
        .collect()
}

fn run_next(cli: &Cli, args: &NextArgs) -> Result<()> {
    let files = load_next_todo_files(cli, args)?;
    let matches = next_actions(&files, args.filter.as_deref(), args.first_available)?;
    let style = OutputStyle {
        color: output_color_enabled(cli),
        na_tag: "na",
    };
    let file_labels = build_filename_labels(&matches);

    for action in matches {
        let file_prefix = file_labels.get(&action.source_file).map(String::as_str);
        println!("{}", format_action(&action, style, file_prefix));
    }

    Ok(())
}

fn load_next_todo_files(cli: &Cli, args: &NextArgs) -> Result<Vec<TodoFile>> {
    if let Some(path) = &args.file {
        return TodoFile::load(path)
            .with_context(|| format!("Failed to read {:?}", path))
            .map(|todo| vec![todo]);
    }

    let depth = args.depth.unwrap_or(5);
    let mut files = if let Some(path) = &cli.global_file {
        vec![path.clone()]
    } else {
        discover_taskpaper_files_with_options(&cli.extension, depth, args.hidden)?
    };

    if !args.in_todo.is_empty() {
        let needles: Vec<String> = args.in_todo.iter().map(|s| s.to_ascii_lowercase()).collect();
        files.retain(|path| {
            let candidate = path.to_string_lossy().to_ascii_lowercase();
            needles.iter().any(|needle| candidate.contains(needle))
        });
    }

    files
        .iter()
        .map(|path| TodoFile::load(path).with_context(|| format!("Failed to read {:?}", path)))
        .collect()
}

fn next_actions(files: &[TodoFile], filter: Option<&str>, first_available: bool) -> Result<Vec<Action>> {
    let query = filter
        .map(Query::parse)
        .transpose()?
        .unwrap_or_else(|| Query::next_defaults("na"));

    let mut matches = evaluate_query(files, &query);
    if first_available {
        // Ruby parity: in filtered mode we should not force @na, but default mode should.
        let require_na = filter.is_none();
        matches = crate::models::actions::first_available_per_project(matches, require_na, "na");
    }

    Ok(matches)
}

fn run_find(cli: &Cli, query: &str) -> Result<()> {
    let files = load_todo_files(cli)?;
    let style = OutputStyle {
        color: output_color_enabled(cli),
        na_tag: "na",
    };
    let matches = find_actions(&files, query)?;
    let file_labels = build_filename_labels(&matches);
    for action in matches {
        let file_prefix = file_labels.get(&action.source_file).map(String::as_str);
        println!("{}", format_action(&action, style, file_prefix));
    }
    Ok(())
}

fn output_color_enabled(cli: &Cli) -> bool {
    !cli.no_color && std::io::stdout().is_terminal()
}

fn build_filename_labels(actions: &[Action]) -> HashMap<String, String> {
    let files: HashSet<&str> = actions.iter().map(|a| a.source_file.as_str()).collect();
    if files.len() <= 1 {
        return HashMap::new();
    }

    let cwd = std::env::current_dir().ok();
    let has_subdir = files.iter().any(|source| {
        let path = Path::new(source);
        if let Some(cwd) = &cwd {
            if let Ok(relative) = path.strip_prefix(cwd) {
                return relative.components().count() > 1;
            }
        }
        path.components().count() > 1
    });

    actions
        .iter()
        .map(|action| {
            let path = Path::new(&action.source_file);
            let label = abbreviate_source_path(path, cwd.as_deref(), has_subdir);
            (action.source_file.clone(), label)
        })
        .collect()
}

fn abbreviate_source_path(path: &Path, cwd: Option<&Path>, show_cwd_indicator: bool) -> String {
    if let Some(cwd) = cwd {
        if let Ok(relative) = path.strip_prefix(cwd) {
            let rel = relative.to_string_lossy().to_string();
            if relative.components().count() <= 1 && show_cwd_indicator {
                return format!("./{rel}");
            }
            return rel;
        }
    }

    let mut out = path.to_string_lossy().to_string();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let home_str = home.to_string_lossy();
        if out.starts_with(home_str.as_ref()) {
            out = out.replacen(home_str.as_ref(), "~", 1);
        }
    }
    out
}

fn find_actions(files: &[TodoFile], query: &str) -> Result<Vec<Action>> {
    let query = Query::parse(query)?;
    Ok(evaluate_query(files, &query))
}

fn run_add(cli: &Cli, text: &str) -> Result<()> {
    let mut files = load_todo_files(cli)?;
    let target = files
        .first_mut()
        .context("No TaskPaper file found; create one or provide --global-file")?;

    target.add_inbox_action(text);
    target.save()?;
    println!("Added action to {:?}", target.path);
    Ok(())
}

fn run_update(cli: &Cli, args: &UpdateArgs) -> Result<()> {
    let mut files = load_todo_files(cli)?;
    let query = Query::parse(&args.query)?;
    let mut updated_count = 0;

    for file in &mut files {
        updated_count += file.apply_update(&query, &args.tag, &args.untag, args.done)?;
    }

    println!("Updated {updated_count} action(s)");
    Ok(())
}

fn run_plugin(cli: &Cli, command: &PluginCommands) -> Result<()> {
    let plugin_dir = PluginRegistry::default_dir()?;
    let registry = PluginRegistry::discover(&plugin_dir)?;

    match command {
        PluginCommands::List => {
            for plugin in &registry.plugins {
                println!("{} ({})", plugin.name, plugin.path.display());
            }
            Ok(())
        }
        PluginCommands::Run { plugin, query } => {
            let files = load_todo_files(cli)?;
            let q = Query::parse(query)?;
            let actions = evaluate_query(&files, &q);
            let runner = registry.plugin(plugin)?;
            let output = runner.run(&actions)?;
            println!("{output}");
            Ok(())
        }
    }
}

#[allow(dead_code)]
fn _single_target(cli: &Cli) -> Option<PathBuf> {
    cli.global_file.clone()
}

#[cfg(test)]
mod tests {
    use super::{abbreviate_source_path, find_actions, load_next_todo_files, next_actions};
    use crate::cli::{Cli, Commands, NextArgs};
    use crate::models::todo::TodoFile;
    use clap::Parser;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn write_fixture_taskpaper(content: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("valid monotonic time")
            .as_nanos();
        let seq = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        path.push(format!(
            "na_rust_next_fixture_{}_{}_{}.taskpaper",
            std::process::id(),
            ts,
            seq
        ));
        fs::write(&path, content).expect("fixture should write");
        path
    }

    #[test]
    fn next_default_requires_na_and_excludes_archive() {
        let path = write_fixture_taskpaper(
            r#"Inbox:
- Keep me @na
- Skip me
Archive:
- Archived @na
"#,
        );
        let todo = TodoFile::load(&path).expect("fixture should load");
        let out = next_actions(&[todo], None, false).expect("next should evaluate");
        fs::remove_file(path).ok();

        let texts = out.into_iter().map(|a| a.text).collect::<Vec<_>>();
        assert_eq!(texts.len(), 1);
        assert!(texts[0].contains("Keep me"));
    }

    #[test]
    fn next_search_with_comparison_filters_expected_actions() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Deep work @priority(5) @context(home-office)
- Email @priority(2) @context(office)
"#,
        );
        let todo = TodoFile::load(&path).expect("fixture should load");
        let out = next_actions(
            &[todo],
            Some(r#"@search(@priority > 3 and @context contains "home")"#),
            false,
        )
        .expect("next should evaluate");
        fs::remove_file(path).ok();

        assert_eq!(out.len(), 1);
        assert!(out[0].text.contains("Deep work"));
    }

    #[test]
    fn next_first_available_respects_filtered_mode_without_forced_na() {
        let path = write_fixture_taskpaper(
            r#"ProjectA:
- First A no tag @context(home)
- Second A @na @context(home)
ProjectB:
- First B no tag @context(home)
"#,
        );
        let todo = TodoFile::load(&path).expect("fixture should load");
        let out = next_actions(
            &[todo],
            Some(r#"@search(@context contains "home")"#),
            true,
        )
        .expect("next should evaluate");
        fs::remove_file(path).ok();

        assert_eq!(out.len(), 2);
        assert!(out[0].text.contains("First A no tag"));
        assert!(out[1].text.contains("First B no tag"));
    }

    #[test]
    fn find_simple_tag_query_uses_shared_query_engine() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Deep work @home
- Email @office
"#,
        );
        let todo = TodoFile::load(&path).expect("fixture should load");
        let out = find_actions(&[todo], "@home").expect("find should evaluate");
        fs::remove_file(path).ok();

        assert_eq!(out.len(), 1);
        assert!(out[0].text.contains("Deep work"));
    }

    #[test]
    fn find_search_query_respects_comparison_filters() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Deep work @priority(5)
- Email @priority(1)
"#,
        );
        let todo = TodoFile::load(&path).expect("fixture should load");
        let out =
            find_actions(&[todo], r#"@search(@priority > 3)"#).expect("find should evaluate");
        fs::remove_file(path).ok();

        assert_eq!(out.len(), 1);
        assert!(out[0].text.contains("Deep work"));
    }

    #[test]
    fn find_done_tag_query_includes_done_items() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Done thing @done
- Active thing
"#,
        );
        let todo = TodoFile::load(&path).expect("fixture should load");
        let out = find_actions(&[todo], "@done").expect("find should evaluate");
        fs::remove_file(path).ok();

        assert_eq!(out.len(), 1);
        assert!(out[0].text.contains("Done thing"));
    }

    #[test]
    fn next_file_option_loads_only_target_file() {
        let path = write_fixture_taskpaper(
            r#"Inbox:
- Keep me @na
"#,
        );
        let cli = Cli::parse_from(["na", "next"]);
        let args = NextArgs {
            filter: None,
            first_available: false,
            file: Some(path.clone()),
            all: false,
            hidden: false,
            depth: None,
            in_todo: Vec::new(),
        };

        let files = load_next_todo_files(&cli, &args).expect("next file loading should work");
        fs::remove_file(path).ok();

        assert_eq!(files.len(), 1);
    }

    #[test]
    fn next_available_alias_invokes_next_command() {
        let cli = Cli::parse_from(["na", "next", "--available"]);
        match cli.command {
            Commands::Next(args) => assert!(args.first_available),
            _ => panic!("expected next command"),
        }
    }

    #[test]
    fn abbreviate_source_path_uses_relative_and_cwd_indicator() {
        let cwd = Path::new("/tmp/workspace");
        let root_file = Path::new("/tmp/workspace/todo.taskpaper");
        let nested_file = Path::new("/tmp/workspace/sub/todo.taskpaper");

        let root = abbreviate_source_path(root_file, Some(cwd), true);
        let nested = abbreviate_source_path(nested_file, Some(cwd), true);

        assert_eq!(root, "./todo.taskpaper");
        assert_eq!(nested, "sub/todo.taskpaper");
    }
}
