use crate::cli::{
    AddArgs, ArchiveArgs, Cli, Commands, CompletedArgs, EditArgs, FindArgs, InitConfigArgs,
    MoveArgs, NextArgs,
    OpenArgs, PluginCommands, ProjectsArgs, PromptArgs, PromptCommands, SavedCommands, ScanArgs,
    TagArgs, TaggedArgs, TodosArgs, UndoArgs, UpdateArgs,
};
use crate::io::fs::{discover_taskpaper_files, discover_taskpaper_files_with_options};
use crate::io::config::{find_na_rc_path, rc_globals_from_cli, write_na_rc};
use crate::io::xdg::{na_backup_dir, na_data_dir};
use crate::models::action::Action;
use crate::models::todo::{TodoFile, UpdateMutation};
use crate::output::duration::{
    accumulate_timing_totals_by_tag, action_timing_window, format_ruby_duration,
    render_duration_footer, serialize_json_times,
};
use crate::output::formatter::{
    color_action_body, format_action, nested_bracketed_chain, paint_themed, visual_width_tabs8,
    wrap_words, OutputStyle,
};
use crate::output::theme::Theme;
use crate::parser::item_path::resolve_item_path;
use crate::parser::search::{evaluate_query, Query};
use crate::parser::{expand_date_tags_in_line, parse_tag_datetime};
use crate::plugins::apply::apply_plugin_stdout_to_files;
use crate::plugins::format::{merge_plugin_stdout_into_actions, PluginDataFormat};
use crate::plugins::registry::{Plugin, PluginRegistry, PluginRunner};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use inquire::{Confirm, InquireError, MultiSelect, Select, Text};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::IsTerminal;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn cli_na_tag(cli: &Cli) -> &str {
    cli.na_tag.trim_start_matches('@')
}

fn effective_na_tag<'a>(cli: &'a Cli, cmd_tag: Option<&'a str>) -> &'a str {
    cmd_tag
        .map(|t| t.trim_start_matches('@'))
        .unwrap_or_else(|| cli_na_tag(cli))
}

fn effective_add_at<'a>(cli: &'a Cli, cmd_at: Option<&'a str>) -> &'a str {
    cmd_at.unwrap_or(cli.add_at.as_str())
}

fn effective_discovery_depth(cli: &Cli, cmd_depth: Option<usize>, default: usize) -> usize {
    cmd_depth.or(cli.depth).unwrap_or(default)
}

fn effective_discovery_depth_usize(cli: &Cli, cmd_depth: usize, default: usize) -> usize {
    if cmd_depth != default {
        cmd_depth
    } else {
        cli.depth.unwrap_or(default)
    }
}

fn is_taskpaper_search_filter(args: &NextArgs) -> bool {
    args.filter
        .as_deref()
        .is_some_and(|f| f.trim().starts_with("@search("))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CwdAsMode {
    None,
    Project,
    Tag,
}

fn parse_cwd_as(raw: &str) -> CwdAsMode {
    let lower = raw.trim().to_ascii_lowercase();
    if lower.starts_with('p') {
        CwdAsMode::Project
    } else if lower.starts_with('t') {
        CwdAsMode::Tag
    } else {
        CwdAsMode::None
    }
}

fn cwd_basename() -> Option<String> {
    std::env::current_dir().ok().and_then(|p| {
        p.file_name()
            .map(|s| s.to_string_lossy().to_string())
    })
}

/// Ruby `NA.add_action`: with `--file` global, cwd basename becomes project or tag.
fn apply_global_file_add_context(cli: &Cli, project: &str, action_text: &str) -> (String, String) {
    if cli.global_file.is_none() {
        return (project.to_string(), action_text.to_string());
    }
    let Some(cwd) = cwd_basename() else {
        return (project.to_string(), action_text.to_string());
    };
    match parse_cwd_as(&cli.cwd_as) {
        CwdAsMode::Tag => {
            let mut text = action_text.to_string();
            if !contains_tag(&text, &cwd) {
                text.push_str(&format!(" @{cwd}"));
            }
            (project.to_string(), text)
        }
        CwdAsMode::Project | CwdAsMode::None => (cwd, action_text.to_string()),
    }
}

fn prompt_hook_command(cli: &Cli) -> Result<String> {
    if cli.global_file.is_some() {
        return match parse_cwd_as(&cli.cwd_as) {
            CwdAsMode::Project => Ok(r#"na next --project "$(basename "$PWD")""#.to_string()),
            CwdAsMode::Tag => Ok(r#"na tagged "$(basename "$PWD")""#.to_string()),
            CwdAsMode::None => anyhow::bail!(
                "When using a global file, a prompt hook requires `--cwd_as [tag|project]`"
            ),
        };
    }
    Ok("na next".to_string())
}

fn prompt_hook_script(cli: &Cli) -> Result<String> {
    let cmd = prompt_hook_command(cli)?;
    let shell = std::env::var("SHELL").unwrap_or_default();
    if shell.contains("fish") {
        let fish_cmd = cmd
            .replace(r#"$(basename "$PWD")"#, "(basename \"$PWD\")");
        Ok(format!(
            "# Fish Prompt Command\nfunction __should_na --on-variable PWD\n  test -s (basename $PWD).{} && {}\nend\n",
            cli.extension, fish_cmd
        ))
    } else if shell.contains("zsh") {
        Ok(format!("# zsh prompt hook for na\nchpwd() {{ {cmd} }}\n"))
    } else {
        Ok(format!(
            "# Bash PROMPT_COMMAND for na\nlast_command_was_cd() {{\n  [[ $(history 1|sed -e \"s/^[ ]*[0-9]*[ ]*//\") =~ ^((cd|z|j|jump|g|f|pushd|popd|exit)([ ]|$)) ]] && {cmd}\n}}\nif [[ -z \"$PROMPT_COMMAND\" ]]; then\n  PROMPT_COMMAND=\"eval 'last_command_was_cd'\"\nelse\n  echo $PROMPT_COMMAND | grep -v -q \"last_command_was_cd\" && PROMPT_COMMAND=\"$PROMPT_COMMAND;\"'eval \"last_command_was_cd\"'\nfi\n"
        ))
    }
}

pub fn run(cli: Cli) -> Result<()> {
    if cli.version {
        println!("na (Rust version) {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    match &cli.command {
        Some(Commands::Next(args)) => run_next(&cli, args),
        Some(Commands::Find(args)) => run_find(&cli, args),
        Some(Commands::Tagged(args)) => run_tagged(&cli, args),
        Some(Commands::Add(args)) => run_add(&cli, args),
        Some(Commands::Update(args)) => run_update(&cli, args),
        Some(Commands::Edit(args)) => run_edit(&cli, args),
        Some(Commands::Complete(args)) => {
            let mut complete = args.clone();
            complete.done = true;
            run_update(&cli, &complete)
        }
        Some(Commands::Archive(args)) => run_archive(&cli, args),
        Some(Commands::Completed(args)) => run_completed(&cli, args),
        Some(Commands::Restore(args)) => run_restore(&cli, args),
        Some(Commands::Move(args)) => run_move(&cli, args),
        Some(Commands::Tag(args)) => run_tag(&cli, args),
        Some(Commands::Open(args)) => run_open(&cli, args),
        Some(Commands::Projects(args)) => run_projects(&cli, args),
        Some(Commands::Todos(args)) => run_todos(&cli, args),
        Some(Commands::Undo(args)) => run_undo(&cli, args),
        Some(Commands::Scan(args)) => run_scan(&cli, args),
        Some(Commands::Init) => run_init(&cli),
        Some(Commands::Prompt(args)) => run_prompt(&cli, args),
        Some(Commands::Changes) => run_changes(),
        Some(Commands::Saved(args)) => run_saved(&args.command),
        Some(Commands::InitConfig(args)) => run_initconfig(&cli, args),
        Some(Commands::Plugin(args)) => run_plugin(&cli, &args.command),
        None => run_next(&cli, &implicit_next_args()),
    }
}

fn implicit_next_args() -> NextArgs {
    NextArgs {
        filter: None,
        first_available: false,
        file: None,
        all: false,
        hidden: false,
        depth: Some(3),
        in_todo: Vec::new(),
        done: false,
        tag: None,
        project: None,
        tagged: Vec::new(),
        priority: Vec::new(),
        search: Vec::new(),
        regex: false,
        exact: false,
        search_notes: true,
        no_search_notes: false,
        notes: false,
        no_notes: false,
        no_file: false,
        nest: false,
        omnifocus: false,
        plugin: None,
        input: None,
        output: None,
        divider: None,
        times: false,
        human: false,
        only_timed: false,
        json_times: false,
        only_times: false,
        save: None,
    }
}

#[derive(Clone, Copy)]
struct TimeOutputFlags {
    human: bool,
    /// User passed `--times`: show action lines with `[DD:HH:MM:SS]` (or human) suffix when timed.
    inline_times: bool,
    only_times: bool,
    json_times: bool,
}

fn run_next(cli: &Cli, args: &NextArgs) -> Result<()> {
    if let Some(title) = args.save.as_deref() {
        save_next_search(args, title)?;
    }
    let files = load_next_todo_files(cli, args)?;
    let mut display_actions = next_actions(cli, &files, args)?;
    if let Some(merged) = run_next_plugin_merge(args, &display_actions)? {
        display_actions = merged;
    }
    let wants_time_block = args.times || args.json_times || args.only_times || args.only_timed;
    if wants_time_block {
        if args.json_times || !args.nest_for_display() {
            return render_next_time_block(cli, args, &display_actions);
        }
    }
    let style = OutputStyle {
        color: output_color_enabled(cli),
        na_tag: cli_na_tag(cli).to_string(),
        include_notes: args.effective_notes(),
        wrap_width: output_wrap_columns(cli),
    };
    let theme = if style.color {
        Theme::load()
    } else {
        Theme::default()
    };
    let file_labels = if args.no_file {
        HashMap::new()
    } else {
        build_filename_labels(&display_actions)
    };

    if args.nest_for_display() {
        print_next_nested(args, &style, &display_actions, &theme);
    } else {
        for action in &display_actions {
            let file_prefix = file_labels.get(&action.source_file).map(String::as_str);
            println!(
                "{}",
                format_action(action, &style, file_prefix, &theme, args.no_file)
            );
        }
    }

    Ok(())
}

fn save_next_search(args: &NextArgs, title: &str) -> Result<()> {
    let slug = saved_search_slug(title);
    if slug.is_empty() {
        return Ok(());
    }
    let dir = saved_searches_dir();
    fs::create_dir_all(&dir).with_context(|| format!("Failed to create {:?}", dir))?;
    let path = dir.join(format!("{slug}.txt"));
    fs::write(&path, format_saved_search(args))
        .with_context(|| format!("Failed to write {:?}", path))?;
    eprintln!("Saved search to {}", path.display());
    Ok(())
}

fn format_saved_search(args: &NextArgs) -> String {
    let mut parts: Vec<String> = vec!["na".to_string(), "next".to_string()];
    if args.first_available {
        parts.push("--first-available".into());
    }
    if let Some(f) = &args.filter {
        parts.push(f.clone());
    }
    if let Some(p) = &args.file {
        parts.extend(["--file".into(), p.display().to_string()]);
    }
    if args.all {
        parts.push("--all".into());
    }
    if args.hidden {
        parts.push("--hidden".into());
    }
    if let Some(d) = args.depth {
        parts.extend(["--depth".into(), d.to_string()]);
    }
    for t in &args.in_todo {
        parts.extend(["--in".into(), t.clone()]);
    }
    if args.done {
        parts.push("--done".into());
    }
    if let Some(t) = &args.tag {
        parts.extend(["--tag".into(), t.clone()]);
    }
    if let Some(p) = &args.project {
        parts.extend(["--project".into(), p.clone()]);
    }
    for t in &args.tagged {
        parts.extend(["--tagged".into(), t.clone()]);
    }
    for p in &args.priority {
        parts.extend(["--priority".into(), p.clone()]);
    }
    for s in &args.search {
        parts.extend(["--search".into(), s.clone()]);
    }
    if args.regex {
        parts.push("--regex".into());
    }
    if args.exact {
        parts.push("--exact".into());
    }
    if args.no_search_notes {
        parts.push("--no-search-notes".into());
    }
    if args.notes {
        parts.push("--notes".into());
    }
    if args.no_notes {
        parts.push("--no-notes".into());
    }
    if args.no_file {
        parts.push("--no-file".into());
    }
    if args.nest {
        parts.push("--nest".into());
    }
    if args.omnifocus {
        parts.push("--omnifocus".into());
    }
    if let Some(pl) = &args.plugin {
        parts.extend(["--plugin".into(), pl.clone()]);
    }
    if let Some(i) = &args.input {
        parts.extend(["--input".into(), i.clone()]);
    }
    if let Some(o) = &args.output {
        parts.extend(["--output".into(), o.clone()]);
    }
    if let Some(d) = &args.divider {
        parts.extend(["--divider".into(), d.clone()]);
    }
    if args.times {
        parts.push("--times".into());
    }
    if args.human {
        parts.push("--human".into());
    }
    if args.only_timed {
        parts.push("--only-timed".into());
    }
    if args.json_times {
        parts.push("--json-times".into());
    }
    if args.only_times {
        parts.push("--only-times".into());
    }
    parts
        .into_iter()
        .map(|part| shell_quote_token(&part))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote_token(token: &str) -> String {
    if token.is_empty() {
        return "''".to_string();
    }
    let needs_quotes = token.chars().any(|c| {
        c.is_whitespace()
            || matches!(
                c,
                '\'' | '"'
                    | '\\'
                    | '$'
                    | '`'
                    | '!'
                    | '&'
                    | '|'
                    | ';'
                    | '<'
                    | '>'
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
            )
    });
    if !needs_quotes {
        return token.to_string();
    }
    let escaped = token.replace('\'', r#"'\''"#);
    format!("'{escaped}'")
}

fn saved_search_slug(title: &str) -> String {
    let mut out = String::new();
    let mut last_was_sep = false;
    for ch in title.chars().flat_map(|c| c.to_lowercase()) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('_');
            last_was_sep = true;
        }
    }
    out.trim_matches('_').to_string()
}

/// Nested `--nest` / `--omnifocus`: word-wrap body to `$COLUMNS` minus prefix visible width,
/// including when colors are enabled (prefix may contain CSI from `[project]` brackets).
fn print_nested_body_lines(
    plain_body: &str,
    first_line_prefix: &str,
    style: &OutputStyle,
    theme: &Theme,
    tags: &[String],
    notes_hidden_marker: bool,
    has_notes: bool,
) {
    let prefix_cols = visual_width_tabs8(first_line_prefix);
    let flush_line = |i: usize, last_i: usize, chunk: &str| {
        let mut out = if i == 0 {
            format!(
                "{}{}",
                first_line_prefix,
                color_action_body(chunk, tags, style.color, theme)
            )
        } else {
            format!(
                "{}{}",
                " ".repeat(prefix_cols),
                color_action_body(chunk, tags, style.color, theme)
            )
        };
        if notes_hidden_marker && has_notes && i == last_i {
            if style.color {
                out.push_str(&paint_themed("*", &theme.note));
            } else {
                out.push('*');
            }
        }
        println!("{}", out);
    };

    if let Some(cols) = style.wrap_width {
        let avail = cols.saturating_sub(prefix_cols).max(12);
        let chunks = wrap_words(plain_body, avail);
        if chunks.is_empty() {
            return;
        }
        let last_i = chunks.len() - 1;
        for (i, chunk) in chunks.iter().enumerate() {
            flush_line(i, last_i, chunk);
        }
    } else {
        flush_line(0, 0, plain_body);
    }
}

/// Ruby `NA.output_children`: append ` @tags(a,b-c)` for tags other than `due`, `flagged`, `done`
/// (`na_gem/lib/na/actions.rb`). Values use `name-value` like `priority-5`.
/// Ruby nest headers use absolute paths (`NA::Todo` expands `--file`); omnifocus applies `~`.
fn nest_header_source_path(source_file: &str) -> String {
    Path::new(source_file)
        .canonicalize()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| source_file.trim_start_matches("./").to_string())
}

fn nest_header_omnifocus_display(source_file: &str) -> String {
    let abs = nest_header_source_path(source_file);
    if let Ok(home) = std::env::var("HOME") {
        if let Some(rest) = abs.strip_prefix(&home) {
            return format!("~{rest}");
        }
    }
    abs
}

fn nest_action_file_header(path: &str, line_idx: usize) -> String {
    format!("{path}:{line_idx}:")
}

fn omnifocus_auxiliary_tags_suffix(action: &Action) -> String {
    const EXCLUDED: &[&str] = &["due", "flagged", "done"];
    let mut parts: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for tag in &action.tags {
        let base = tag
            .strip_prefix('@')
            .and_then(|s| s.split('(').next())
            .unwrap_or(tag.as_str());
        let key_lc = base.to_ascii_lowercase();
        if EXCLUDED.iter().any(|e| *e == key_lc.as_str()) {
            continue;
        }
        if !seen.insert(key_lc) {
            continue;
        }
        let val = action
            .tag_values
            .get(&base.to_ascii_lowercase())
            .map(String::as_str)
            .unwrap_or("");
        let segment = if val.is_empty() {
            base.to_string()
        } else {
            format!("{base}-{val}")
        };
        parts.push(segment);
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" @tags({})", parts.join(","))
    }
}

fn print_next_nested(args: &NextArgs, style: &OutputStyle, matches: &[Action], theme: &Theme) {
    // Ruby `NA::Actions.output` nest mode: file headers and `\t- [#{parent}] #{action}` / omni `output_children`
    // use **full** action text (see `na_gem/lib/na/actions.rb`). Flat `Action#pretty` alone strips `@na`.
    #[derive(Default)]
    struct OmniNode<'a> {
        children: BTreeMap<String, OmniNode<'a>>,
        actions: Vec<&'a Action>,
    }

    fn omni_insert<'a>(node: &mut OmniNode<'a>, chain: &[String], action: &'a Action) {
        if chain.is_empty() {
            node.actions.push(action);
            return;
        }
        let (head, rest) = chain.split_first().expect("non-empty chain");
        let child = node
            .children
            .entry(head.clone())
            .or_insert_with(OmniNode::default);
        if rest.is_empty() {
            child.actions.push(action);
        } else {
            omni_insert(child, rest, action);
        }
    }

    fn print_omni_tree(node: &OmniNode<'_>, level: usize, style: &OutputStyle, theme: &Theme) {
        let indent = "\t".repeat(level);
        // Ruby `NA.output_children`: an `:actions` bucket is handled before sibling project keys,
        // which advances `indent` by one tab even when that bucket is empty.
        let branch_indent = format!("{indent}\t");
        for (name, child) in &node.children {
            let header = if style.color {
                format!(
                    "{branch_indent}{}",
                    paint_themed(&format!("{name}:"), &theme.project)
                )
            } else {
                format!("{branch_indent}{name}:")
            };
            println!("{}", header);
            print_omni_tree(child, level + 1, style, theme);
        }
        if !node.actions.is_empty() {
            // Action lines use the same indent Ruby leaves after processing `:actions`
            // (`item = "#{indent}- #{a.action}"` — no extra tab vs project headers).
            let line_indent = branch_indent.clone();
            for a in &node.actions {
                let plain = format!("{}{}", a.text, omnifocus_auxiliary_tags_suffix(a));
                let lead = format!("{line_indent}- ");
                print_nested_body_lines(
                    &plain,
                    &lead,
                    style,
                    theme,
                    &a.tags,
                    !style.include_notes,
                    !a.notes.is_empty(),
                );
                if style.include_notes {
                    for n in &a.notes {
                        if style.color {
                            println!("{line_indent}\t{}", paint_themed(n, &theme.note));
                        } else {
                            println!("{line_indent}\t{n}");
                        }
                    }
                }
            }
        }
    }

    // Mirror Ruby `NA::Actions.output`: group key is `path:line` per action, so each action gets
    // its own `path:line:` banner (see `NA::Action#initialize`).
    for action in matches {
        let header_path = if args.omnifocus {
            nest_header_omnifocus_display(&action.source_file)
        } else {
            nest_header_source_path(&action.source_file)
        };
        println!(
            "{}",
            nest_action_file_header(&header_path, action.line_index)
        );

        if args.omnifocus {
            let mut root = OmniNode::default();
            omni_insert(&mut root, &action.project_chain, action);
            print_omni_tree(&root, 0, style, theme);
        } else {
            let chain = action.project_chain.join("/");
            let bracket = nested_bracketed_chain(&chain, style.color, theme);
            let plain = action.text.clone();
            let lead = format!("\t- {bracket} ");
            print_nested_body_lines(
                &plain,
                &lead,
                style,
                theme,
                &action.tags,
                !style.include_notes,
                !action.notes.is_empty(),
            );
            if style.include_notes {
                for note in &action.notes {
                    if style.color {
                        println!("\t\t{}", paint_themed(note, &theme.note));
                    } else {
                        println!("\t\t{note}");
                    }
                }
            }
        }
    }
}

fn run_next_plugin_merge(args: &NextArgs, actions: &[Action]) -> Result<Option<Vec<Action>>> {
    let Some(name) = args.plugin.as_deref() else {
        return Ok(None);
    };
    let dir = PluginRegistry::default_dir()?;
    let registry = PluginRegistry::discover(&dir)?;
    let plugin = registry.resolve_plugin(name)?;
    let runner = PluginRunner::new(plugin.clone());
    let input = args.input.as_deref().and_then(PluginDataFormat::parse);
    let output = args.output.as_deref().and_then(PluginDataFormat::parse);
    let divider = args.divider.as_deref();
    let stdout = runner.run_with_formats(actions, input, output, divider)?;
    let out_fmt = output.unwrap_or(plugin.output_format);
    let merged = merge_plugin_stdout_into_actions(actions, &stdout, out_fmt, divider)?;
    Ok(Some(merged))
}

fn load_next_todo_files(cli: &Cli, args: &NextArgs) -> Result<Vec<TodoFile>> {
    if let Some(path) = &args.file {
        return TodoFile::load(path)
            .with_context(|| format!("Failed to read {:?}", path))
            .map(|todo| vec![todo]);
    }

    let depth = effective_discovery_depth(cli, args.depth, 5);
    let mut files = if let Some(path) = &cli.global_file {
        vec![path.clone()]
    } else {
        discover_taskpaper_files_with_options(&cli.extension, depth, args.hidden)?
    };

    if !args.in_todo.is_empty() {
        let needles: Vec<String> = args
            .in_todo
            .iter()
            .map(|s| s.to_ascii_lowercase())
            .collect();
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

fn next_actions(cli: &Cli, files: &[TodoFile], args: &NextArgs) -> Result<Vec<Action>> {
    let include_done_for_time = args.times || args.json_times || args.only_times || args.only_timed;
    let query = build_next_query(cli, args)?.with_include_done(args.done || include_done_for_time);

    let mut matches = evaluate_query(files, &query);
    matches = apply_next_search_filter(matches, args)?;
    if args.only_timed {
        matches.retain(|a| action_timing_window(a).is_some());
    }
    if args.first_available && !is_taskpaper_search_filter(args) {
        let require_na = next_requires_na(args);
        matches =
            crate::models::actions::first_available_per_project(matches, require_na, cli_na_tag(cli));
    }

    Ok(matches)
}

fn render_next_time_block(cli: &Cli, args: &NextArgs, actions: &[Action]) -> Result<()> {
    let style = OutputStyle {
        color: output_color_enabled(cli),
        na_tag: cli_na_tag(cli).to_string(),
        include_notes: args.effective_notes(),
        wrap_width: output_wrap_columns(cli),
    };
    let theme = if style.color {
        Theme::load()
    } else {
        Theme::default()
    };
    let file_labels = if args.no_file {
        HashMap::new()
    } else {
        build_filename_labels(actions)
    };
    let flags = TimeOutputFlags {
        human: args.human,
        inline_times: (args.times || args.only_timed) && !args.only_times,
        only_times: args.only_times,
        json_times: args.json_times,
    };
    render_actions_time_summary(actions, flags, &style, &theme, &file_labels, args.no_file)
}

fn render_find_time_block(cli: &Cli, args: &FindArgs, actions: &[Action]) -> Result<()> {
    let style = OutputStyle {
        color: output_color_enabled(cli),
        na_tag: cli_na_tag(cli).to_string(),
        include_notes: args.effective_notes(),
        wrap_width: output_wrap_columns(cli),
    };
    let theme = if style.color {
        Theme::load()
    } else {
        Theme::default()
    };
    let file_labels = if args.no_file {
        HashMap::new()
    } else {
        build_filename_labels(actions)
    };
    let flags = TimeOutputFlags {
        human: args.human,
        inline_times: (args.times || args.only_timed) && !args.only_times,
        only_times: args.only_times,
        json_times: args.json_times,
    };
    render_actions_time_summary(actions, flags, &style, &theme, &file_labels, args.no_file)
}

fn render_actions_time_summary(
    actions: &[Action],
    flags: TimeOutputFlags,
    style: &OutputStyle,
    theme: &Theme,
    file_labels: &HashMap<String, String>,
    omit_filename: bool,
) -> Result<()> {
    let mut totals_by_tag: HashMap<String, i64> = HashMap::new();
    let mut total_seconds: i64 = 0;
    for action in actions {
        if let Some((_, _, secs)) = action_timing_window(action) {
            total_seconds += secs;
            accumulate_timing_totals_by_tag(action, secs, &mut totals_by_tag);
        }
    }

    if flags.json_times {
        println!(
            "{}",
            serialize_json_times(actions, &totals_by_tag, total_seconds)?
        );
        return Ok(());
    }

    for action in actions {
        if flags.only_times {
            continue;
        }
        let fp = file_labels.get(&action.source_file).map(String::as_str);
        let mut line = format_action(&action, &style, fp, theme, omit_filename);
        if flags.inline_times {
            if let Some((_, _, secs)) = action_timing_window(action) {
                line.push_str(" [");
                line.push_str(&format_ruby_duration(secs, flags.human));
                line.push_str("]");
            }
        }
        println!("{}", line);
    }

    let show_footer = flags.inline_times || flags.only_times;
    if show_footer && total_seconds > 0 {
        let mut buf = String::new();
        render_duration_footer(&mut buf, total_seconds, flags.human, &totals_by_tag)
            .map_err(|_| anyhow::anyhow!("time footer"))?;
        print!("{}", buf);
    }
    Ok(())
}

fn build_next_query(cli: &Cli, args: &NextArgs) -> Result<Query> {
    if let Some(filter) = args.filter.as_deref() {
        return Query::parse(filter);
    }
    if args.tag.is_none()
        && args.project.is_none()
        && args.tagged.is_empty()
        && args.priority.is_empty()
        && !args.done
        && !args.times
        && !args.only_timed
        && !args.json_times
        && !args.only_times
    {
        return Ok(Query::next_defaults(cli_na_tag(cli)));
    }

    let na_tag = effective_na_tag(cli, args.tag.as_deref());
    let mut predicates: Vec<String> = Vec::new();
    if next_requires_na(args) {
        predicates.push(format!("@{na_tag}"));
    }
    let done_enabled =
        args.done || args.times || args.only_timed || args.json_times || args.only_times;
    if !done_enabled {
        predicates.push("not @done".to_string());
    }
    predicates.push("not project = \"Archive\"".to_string());
    if let Some(project) = &args.project {
        predicates.push(format!("project = \"{project}\""));
    }
    for tagged in &args.tagged {
        predicates.push(normalize_tagged_predicate(tagged));
    }
    for priority in &args.priority {
        predicates.push(normalize_priority_predicate(priority));
    }

    Query::parse(&format!("@search({})", predicates.join(" and ")))
}

fn next_requires_na(args: &NextArgs) -> bool {
    let filtered_mode = next_filtered_mode(args);
    !(args.first_available && filtered_mode)
}

fn next_filtered_mode(args: &NextArgs) -> bool {
    args.all
        || args.tag.is_some()
        || args.project.is_some()
        || !args.tagged.is_empty()
        || !args.priority.is_empty()
        || !args.search.is_empty()
        || args.save.is_some()
}

fn normalize_tagged_predicate(raw: &str) -> String {
    let mut token = raw.trim().trim_start_matches('@').to_string();
    if let Some(open) = token.find('(') {
        if token.ends_with(')') {
            let tag = token[..open].trim();
            let val = token[open + 1..token.len() - 1].trim();
            token = format!("{tag}={val}");
        }
    }
    format!("@{token}")
}

fn normalize_priority_predicate(raw: &str) -> String {
    let val = raw.trim().to_ascii_lowercase();
    let mapped = match val.as_str() {
        "h" => "9".to_string(),
        "m" => "5".to_string(),
        "l" => "1".to_string(),
        _ => val.clone(),
    };
    if mapped.starts_with('>') || mapped.starts_with('<') || mapped.starts_with('=') {
        format!("@priority{mapped}")
    } else {
        format!("@priority={mapped}")
    }
}

fn apply_next_search_filter(mut actions: Vec<Action>, args: &NextArgs) -> Result<Vec<Action>> {
    if args.search.is_empty() {
        return Ok(actions);
    }
    let haystack = |a: &Action| {
        let mut value = a.text.clone();
        if args.effective_search_notes() {
            value.push('\n');
            value.push_str(&a.notes.join("\n"));
        }
        value
    };
    if args.regex {
        let pattern = args.search.join(" ");
        let rx = regex::RegexBuilder::new(&pattern)
            .case_insensitive(true)
            .build()?;
        actions.retain(|a| rx.is_match(&haystack(a)));
        return Ok(actions);
    }
    let lowered_needles: Vec<String> = if args.exact {
        vec![args.search.join(" ").to_ascii_lowercase()]
    } else {
        args.search
            .join(" ")
            .split_whitespace()
            .map(|t| t.to_ascii_lowercase())
            .collect()
    };
    actions.retain(|a| {
        let text = haystack(a).to_ascii_lowercase();
        lowered_needles.iter().all(|needle| text.contains(needle))
    });
    Ok(actions)
}

fn run_find(cli: &Cli, args: &FindArgs) -> Result<()> {
    if args.query.trim().is_empty() {
        anyhow::bail!("find requires a search pattern (try `na tagged ...` for tag-only filters)");
    }
    if let Some(title) = args.save.as_deref() {
        let mut next_args = implicit_next_args();
        next_args.filter = Some(args.query.clone());
        save_next_search(&next_args, title)?;
    }
    let files = load_find_todo_files(cli, args)?;
    let mut matches = find_actions_with_options(&files, args)?;
    let effective_done = args.done || args.json_times || args.only_times || args.only_timed;
    if !effective_done {
        matches.retain(|a| !a.done);
    }
    if args.only_timed {
        matches.retain(|a| action_timing_window(a).is_some());
    }
    if let Some(project) = &args.project {
        let needle = project.to_ascii_lowercase();
        matches.retain(|a| {
            a.project_chain
                .iter()
                .any(|p| p.to_ascii_lowercase().contains(&needle))
        });
    }
    if !args.tagged.is_empty() {
        matches.retain(|a| {
            args.tagged.iter().all(|t| {
                let tag = if t.starts_with('@') {
                    t.to_ascii_lowercase()
                } else {
                    format!("@{}", t.to_ascii_lowercase())
                };
                a.tags.iter().any(|x| x.eq_ignore_ascii_case(&tag))
            })
        });
    }
    if args.invert {
        let all: Vec<Action> = files.iter().flat_map(TodoFile::actions).collect();
        let set: HashSet<(String, usize)> = matches
            .iter()
            .map(|a| (a.source_file.clone(), a.line_index))
            .collect();
        matches = all
            .into_iter()
            .filter(|a| !set.contains(&(a.source_file.clone(), a.line_index)))
            .collect();
    }
    if let Some(merged) = run_find_plugin_merge(args, &matches)? {
        matches = merged;
    }

    let wants_time_block = args.times || args.json_times || args.only_times || args.only_timed;
    if wants_time_block && (args.json_times || !args.nest_for_display()) {
        return render_find_time_block(cli, args, &matches);
    }

    let style = OutputStyle {
        color: output_color_enabled(cli),
        na_tag: cli_na_tag(cli).to_string(),
        include_notes: args.effective_notes(),
        wrap_width: output_wrap_columns(cli),
    };
    let theme = if style.color {
        Theme::load()
    } else {
        Theme::default()
    };
    let file_labels = if args.no_file {
        HashMap::new()
    } else {
        build_filename_labels(&matches)
    };
    if args.nest_for_display() {
        print_next_nested(
            &NextArgs {
                nest: args.nest,
                omnifocus: args.omnifocus,
                ..implicit_next_args()
            },
            &style,
            &matches,
            &theme,
        );
    } else {
        for action in matches {
            let file_prefix = file_labels.get(&action.source_file).map(String::as_str);
            println!(
                "{}",
                format_action(&action, &style, file_prefix, &theme, args.no_file)
            );
        }
    }
    Ok(())
}

fn run_find_plugin_merge(args: &FindArgs, actions: &[Action]) -> Result<Option<Vec<Action>>> {
    let Some(name) = args.plugin.as_deref() else {
        return Ok(None);
    };
    let dir = PluginRegistry::default_dir()?;
    let registry = PluginRegistry::discover(&dir)?;
    let plugin = registry.resolve_plugin(name)?;
    let runner = PluginRunner::new(plugin.clone());
    let input = args.input.as_deref().and_then(PluginDataFormat::parse);
    let output = args.output.as_deref().and_then(PluginDataFormat::parse);
    let divider = args.divider.as_deref();
    let stdout = runner.run_with_formats(actions, input, output, divider)?;
    let out_fmt = output.unwrap_or(plugin.output_format);
    let merged = merge_plugin_stdout_into_actions(actions, &stdout, out_fmt, divider)?;
    Ok(Some(merged))
}

fn load_find_todo_files(cli: &Cli, args: &FindArgs) -> Result<Vec<TodoFile>> {
    let depth = effective_discovery_depth(cli, args.depth, 5);
    let mut files = if let Some(path) = &cli.global_file {
        vec![path.clone()]
    } else {
        discover_taskpaper_files_with_options(&cli.extension, depth, false)?
    };
    if !args.in_todo.is_empty() {
        let specs = parse_todo_specs(&args.in_todo);
        files.retain(|path| match_todo_path(path.to_string_lossy().as_ref(), &specs));
    }
    files
        .iter()
        .map(|path| TodoFile::load(path).with_context(|| format!("Failed to read {:?}", path)))
        .collect()
}

fn find_actions_with_options(files: &[TodoFile], args: &FindArgs) -> Result<Vec<Action>> {
    let query = args.query.trim();
    if query.starts_with("@search(") {
        return find_actions(files, query);
    }
    let mut out: Vec<Action> = files.iter().flat_map(TodoFile::actions).collect();
    let haystack = |a: &Action| {
        if args.effective_search_notes() {
            format!("{} {}", a.text, a.notes.join(" "))
        } else {
            a.text.clone()
        }
    };
    if args.regex {
        let rx = regex::Regex::new(query)?;
        out.retain(|a| rx.is_match(&haystack(a)));
        return Ok(out);
    }
    let query_lc = query.to_ascii_lowercase();
    if args.exact {
        out.retain(|a| haystack(a).to_ascii_lowercase().contains(&query_lc));
        return Ok(out);
    }
    let tokens: Vec<String> = query
        .split_whitespace()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_ascii_lowercase())
        .collect();
    if tokens.is_empty() {
        return Ok(out);
    }
    if args.or_mode {
        out.retain(|a| {
            let h = haystack(a).to_ascii_lowercase();
            tokens.iter().any(|t| h.contains(t))
        });
    } else {
        out.retain(|a| {
            let h = haystack(a).to_ascii_lowercase();
            tokens.iter().all(|t| h.contains(t))
        });
    }
    Ok(out)
}

/// Builds tag filters then delegates to [`run_find`] so **all** find flags apply (`--no-file`, `--nest`, plugins, etc.).
fn run_tagged(cli: &Cli, args: &TaggedArgs) -> Result<()> {
    let mut find = args.find.clone();
    if !args.tags.is_empty() {
        find.query = args
            .tags
            .iter()
            .map(|t| {
                if t.starts_with('@') {
                    t.clone()
                } else {
                    format!("@{t}")
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
    }
    run_find(cli, &find)
}

fn run_completed(cli: &Cli, args: &CompletedArgs) -> Result<()> {
    if let Some(title) = args.save.as_deref() {
        let mut next_args = implicit_next_args();
        let mut parts = vec!["@done".to_string()];
        if !args.pattern.is_empty() {
            parts.extend(args.pattern.clone());
        }
        next_args.filter = Some(parts.join(" "));
        save_next_search(&next_args, title)?;
    }
    let files = load_completed_todo_files(cli, args)?;
    let mut matches: Vec<Action> = files.iter().flat_map(TodoFile::actions).collect();
    matches.retain(|a| a.done);

    if !args.pattern.is_empty() {
        matches
            .retain(|a| completed_matches_pattern(a, &args.pattern, args.effective_search_notes()));
    }
    if let Some(project) = &args.project {
        let needle = project.to_ascii_lowercase();
        matches.retain(|a| {
            a.project_chain
                .iter()
                .any(|p| p.to_ascii_lowercase().contains(&needle))
        });
    }
    if !args.tagged.is_empty() {
        matches.retain(|a| {
            args.tagged.iter().all(|t| {
                let tag = if t.starts_with('@') {
                    t.to_ascii_lowercase()
                } else {
                    format!("@{}", t.to_ascii_lowercase())
                };
                a.tags.iter().any(|x| x.eq_ignore_ascii_case(&tag))
            })
        });
    }
    if args.before.is_some() || args.on.is_some() || args.after.is_some() {
        let before = args.before.as_deref().and_then(parse_tag_datetime);
        let on = args.on.as_deref().and_then(parse_tag_datetime);
        let after = args.after.as_deref().and_then(parse_tag_datetime);
        matches.retain(|a| completed_matches_date(a, before, on, after, args.or_mode));
    }

    let style = OutputStyle {
        color: output_color_enabled(cli),
        na_tag: cli_na_tag(cli).to_string(),
        include_notes: args.effective_notes(),
        wrap_width: output_wrap_columns(cli),
    };
    let theme = if style.color {
        Theme::load()
    } else {
        Theme::default()
    };
    let file_labels = if args.no_file {
        HashMap::new()
    } else {
        build_filename_labels(&matches)
    };
    if args.nest_for_display() {
        print_next_nested(
            &NextArgs {
                nest: args.nest,
                omnifocus: args.omnifocus,
                ..implicit_next_args()
            },
            &style,
            &matches,
            &theme,
        );
    } else {
        for action in matches {
            let file_prefix = file_labels.get(&action.source_file).map(String::as_str);
            println!(
                "{}",
                format_action(&action, &style, file_prefix, &theme, args.no_file)
            );
        }
    }
    Ok(())
}

fn load_completed_todo_files(cli: &Cli, args: &CompletedArgs) -> Result<Vec<TodoFile>> {
    let depth = effective_discovery_depth(cli, args.depth, 5);
    let mut files = if let Some(path) = &cli.global_file {
        vec![path.clone()]
    } else {
        discover_taskpaper_files_with_options(&cli.extension, depth, false)?
    };
    if !args.in_todo.is_empty() {
        let specs = parse_todo_specs(&args.in_todo);
        files.retain(|path| match_todo_path(path.to_string_lossy().as_ref(), &specs));
    }
    files
        .iter()
        .map(|path| TodoFile::load(path).with_context(|| format!("Failed to read {:?}", path)))
        .collect()
}

fn completed_matches_pattern(action: &Action, patterns: &[String], include_notes: bool) -> bool {
    let haystack = if include_notes {
        format!("{} {}", action.text, action.notes.join(" "))
    } else {
        action.text.clone()
    }
    .to_ascii_lowercase();
    let all_required = !patterns
        .iter()
        .any(|p| p.starts_with('+') || p.starts_with('!') || p.starts_with('-'));
    let mut positives = Vec::new();
    let mut required = Vec::new();
    let mut negatives = Vec::new();
    for p in patterns {
        let trimmed = p.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (prefix, token) = match trimmed.chars().next() {
            Some(ch @ ('+' | '!' | '-')) => (Some(ch), trimmed[1..].trim()),
            _ => (None, trimmed),
        };
        if token.is_empty() {
            continue;
        }
        let token = token.to_ascii_lowercase();
        if matches!(prefix, Some('!' | '-')) {
            negatives.push(token);
        } else if all_required || matches!(prefix, Some('+')) {
            required.push(token.clone());
            positives.push(token);
        } else {
            positives.push(token);
        }
    }
    if negatives.iter().any(|n| haystack.contains(n)) {
        return false;
    }
    if required.iter().any(|r| !haystack.contains(r)) {
        return false;
    }
    let optional: Vec<&String> = positives.iter().filter(|p| !required.contains(p)).collect();
    optional.is_empty() || optional.iter().any(|o| haystack.contains(*o))
}

fn completed_matches_date(
    action: &Action,
    before: Option<DateTime<Utc>>,
    on: Option<DateTime<Utc>>,
    after: Option<DateTime<Utc>>,
    or_mode: bool,
) -> bool {
    let Some(done) = action.tag_value("done").and_then(parse_tag_datetime) else {
        return false;
    };
    let mut checks = Vec::new();
    if let Some(b) = before {
        checks.push(done < b);
    }
    if let Some(o) = on {
        checks.push(done.date_naive() == o.date_naive());
    }
    if let Some(a) = after {
        checks.push(done > a);
    }
    if checks.is_empty() {
        true
    } else if or_mode {
        checks.into_iter().any(|x| x)
    } else {
        checks.into_iter().all(|x| x)
    }
}

fn output_color_enabled(cli: &Cli) -> bool {
    !cli.no_color && std::io::stdout().is_terminal()
}

/// Terminal width from `$COLUMNS` when stdout is a TTY.
///
/// **`format_action` (flat list)** applies word wrap only when color is disabled, so forcing
/// color still yields a single long line there. **`--nest` / `--omnifocus`** use this width for
/// action bodies whenever `COLUMNS` is set, including with theme colors (prefix width skips ANSI).
fn output_wrap_columns(_cli: &Cli) -> Option<usize> {
    if !std::io::stdout().is_terminal() {
        return None;
    }
    std::env::var("COLUMNS").ok()?.parse().ok()
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

fn run_add(cli: &Cli, args: &AddArgs) -> Result<()> {
    let mut files = load_add_todo_files(cli, args)?;
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let file_idx = choose_add_file_index(&files, interactive)?;
    let target = files.get_mut(file_idx).context("No TaskPaper file found")?;
    let project = resolve_add_project(target, &args.project, interactive)?;
    let action_text = build_add_text(cli, args);
    let (project, action_text) = apply_global_file_add_context(cli, &project, &action_text);
    let notes = collect_add_notes(args, &args.text)?;
    let append = add_position_is_append(Some(effective_add_at(cli, args.at.as_deref())));
    target.add_action(Some(&project), &action_text, &notes, append);
    target.save()?;
    println!("Added action to {:?}", target.path);
    Ok(())
}

fn choose_add_file_index(files: &[TodoFile], interactive: bool) -> Result<usize> {
    if files.is_empty() {
        anyhow::bail!("No TaskPaper file found");
    }
    if files.len() == 1 || !interactive {
        return Ok(0);
    }
    let labels: Vec<String> = files.iter().map(|f| f.path.display().to_string()).collect();
    choose_from_menu(
        "Multiple todo files found, select target",
        &labels,
        interactive,
    )
}

fn load_add_todo_files(cli: &Cli, args: &AddArgs) -> Result<Vec<TodoFile>> {
    if let Some(path) = &args.file {
        return TodoFile::load(path)
            .with_context(|| format!("Failed to read {:?}", path))
            .map(|todo| vec![todo]);
    }
    if let Some(path) = &cli.global_file {
        return TodoFile::load(path)
            .with_context(|| format!("Failed to read {:?}", path))
            .map(|todo| vec![todo]);
    }
    let depth = effective_discovery_depth(cli, args.depth, 1);
    let mut files = discover_taskpaper_files_with_options(&cli.extension, depth, false)?;
    if !args.in_todo.is_empty() {
        let specs = parse_todo_specs(&args.in_todo);
        files.retain(|path| match_todo_path(path.to_string_lossy().as_ref(), &specs));
    }
    files
        .iter()
        .map(|path| TodoFile::load(path).with_context(|| format!("Failed to read {:?}", path)))
        .collect()
}

#[derive(Debug, Clone)]
struct TodoSpec {
    token: String,
    required: bool,
    negate: bool,
}

fn parse_todo_specs(raw: &[String]) -> Vec<TodoSpec> {
    let joined = raw.join(",");
    let all_required = !joined.chars().any(|ch| ch == '+' || ch == '!' || ch == '-');
    joined
        .split(',')
        .filter_map(|part| {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                return None;
            }
            let (prefix, token) = match trimmed.chars().next() {
                Some(ch @ ('+' | '!' | '-')) => (Some(ch), trimmed[1..].trim()),
                _ => (None, trimmed),
            };
            if token.is_empty() {
                return None;
            }
            let negate = matches!(prefix, Some('!' | '-'));
            let required = all_required || matches!(prefix, Some('+'));
            Some(TodoSpec {
                token: token.to_ascii_lowercase(),
                required,
                negate,
            })
        })
        .collect()
}

fn match_todo_path(candidate: &str, specs: &[TodoSpec]) -> bool {
    if specs.is_empty() {
        return true;
    }
    let c = candidate.to_ascii_lowercase();
    if specs
        .iter()
        .filter(|s| s.negate)
        .any(|s| c.contains(&s.token))
    {
        return false;
    }
    let positives: Vec<&TodoSpec> = specs.iter().filter(|s| !s.negate).collect();
    if positives.is_empty() {
        return true;
    }
    let required: Vec<&TodoSpec> = positives.iter().copied().filter(|s| s.required).collect();
    if required.iter().any(|s| !c.contains(&s.token)) {
        return false;
    }
    let optional: Vec<&TodoSpec> = positives.iter().copied().filter(|s| !s.required).collect();
    if optional.is_empty() {
        true
    } else {
        optional.iter().any(|s| c.contains(&s.token))
    }
}

fn resolve_add_project(todo: &TodoFile, project: &str, interactive: bool) -> Result<String> {
    if !project.starts_with('/') {
        return Ok(project.to_string());
    }
    let resolved = resolve_item_path(project, &todo.project_paths());
    if resolved.is_empty() {
        return Ok(project.trim_start_matches('/').replace('/', ":"));
    }
    if resolved.len() == 1 || !interactive {
        return Ok(resolved[0].clone());
    }
    let idx = choose_from_menu(
        "Multiple matching projects found, select target",
        &resolved,
        interactive,
    )?;
    Ok(resolved[idx].clone())
}

fn choose_from_menu(prompt: &str, options: &[String], interactive: bool) -> Result<usize> {
    if options.is_empty() {
        anyhow::bail!("No options available for selection");
    }
    if options.len() == 1 || !interactive {
        return Ok(0);
    }
    let picked = Select::new(prompt, options.to_vec())
        .prompt()
        .map_err(map_inquire_error)?;
    options
        .iter()
        .position(|opt| opt == &picked)
        .context("Invalid selection state")
}

#[cfg(test)]
fn parse_one_based_selection(input: &str, len: usize) -> Option<usize> {
    let n = input.trim().parse::<usize>().ok()?;
    if (1..=len).contains(&n) {
        Some(n - 1)
    } else {
        None
    }
}

#[cfg(test)]
fn is_cancel_selection(input: &str) -> bool {
    let trimmed = input.trim().to_ascii_lowercase();
    trimmed.is_empty() || trimmed == "q" || trimmed == "quit"
}

fn build_add_text(cli: &Cli, args: &AddArgs) -> String {
    let mut action = strip_trailing_note(&args.text).0;

    if let Some(priority) = parse_priority_value(args.priority.as_deref()) {
        action = remove_tag_value(&action, "priority");
        action.push_str(&format!(" @priority({priority})"));
    }

    if !args.no_next_tag {
        let next_tag = effective_na_tag(cli, args.tag.as_deref());
        action = remove_tag_value(&action, next_tag);
        action.push_str(&format!(" @{}", next_tag));
    }

    if let Some(started) = args.started.as_deref() {
        action = remove_tag_value(&action, "start");
        action = remove_tag_value(&action, "started");
        action.push_str(&format!(" @started({started})"));
    }
    if let Some(done_at) = args.end.as_deref() {
        action = remove_tag_value(&action, "done");
        action.push_str(&format!(" @done({done_at})"));
    } else if args.finish {
        if !contains_tag(&action, "done") {
            action.push_str(" @done");
        }
    }
    if let Some(duration) = args.duration.as_deref() {
        action = remove_tag_value(&action, "duration");
        action.push_str(&format!(" @duration({duration})"));
    }

    action.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn collect_add_notes(args: &AddArgs, raw_text: &str) -> Result<Vec<String>> {
    let mut notes = Vec::new();
    if let Some(note) = strip_trailing_note(raw_text).1 {
        notes.push(note);
    }
    if args.note && !std::io::stdin().is_terminal() {
        let mut stdin_data = String::new();
        std::io::stdin().read_to_string(&mut stdin_data)?;
        notes.extend(
            stdin_data
                .lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string),
        );
    }
    Ok(notes)
}

fn strip_trailing_note(input: &str) -> (String, Option<String>) {
    let trimmed = input.trim();
    if let Some(open_idx) = trimmed.rfind(" (") {
        if trimmed.ends_with(')') {
            let body = trimmed[..open_idx].trim_end().to_string();
            let note = trimmed[open_idx + 2..trimmed.len() - 1].trim().to_string();
            if !body.is_empty() && !note.is_empty() {
                return (body, Some(note));
            }
        }
    }
    (trimmed.to_string(), None)
}

fn add_position_is_append(pos: Option<&str>) -> bool {
    match pos.map(|s| s.trim().to_ascii_lowercase()) {
        Some(v) if v.starts_with('s') || v.starts_with('b') => false,
        Some(v) if v.starts_with('a') || v.starts_with('e') => true,
        _ => false,
    }
}

fn parse_priority_value(input: Option<&str>) -> Option<u8> {
    let raw = input?.trim().to_ascii_lowercase();
    if let Ok(n) = raw.parse::<u8>() {
        if (1..=5).contains(&n) {
            return Some(n);
        }
    }
    match raw.as_str() {
        "h" => Some(5),
        "m" => Some(3),
        "l" => Some(1),
        _ => None,
    }
}

fn contains_tag(text: &str, tag_name: &str) -> bool {
    let needle = format!("@{}", tag_name.trim_start_matches('@').to_ascii_lowercase());
    let needle_with_open = format!("{needle}(");
    text.split_whitespace()
        .map(|s| s.to_ascii_lowercase())
        .any(|t| t == needle || t.starts_with(&needle_with_open))
}

fn remove_tag_value(text: &str, tag_name: &str) -> String {
    let target = format!("@{}", tag_name.trim_start_matches('@').to_ascii_lowercase());
    text.split_whitespace()
        .filter(|token| {
            let lower = token.to_ascii_lowercase();
            !(lower == target || lower.starts_with(&(target.clone() + "(")))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn run_update(cli: &Cli, args: &UpdateArgs) -> Result<()> {
    if args.archive {
        let archive = ArchiveArgs {
            query: args.query.clone(),
            done: args.done,
            file: args.file.clone(),
            depth: args.depth,
            tagged: args.tagged.clone(),
            project: args.project.clone(),
            in_todo: args.in_todo.clone(),
            search: args.search.clone(),
            regex: args.regex,
            exact: args.exact,
            all: args.all,
            note: args.note.clone(),
            overwrite_notes: args.overwrite_notes,
        };
        return run_archive(cli, &archive);
    }
    let mut files = load_update_todo_files(cli, args)?;
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let matches = collect_update_candidates(&files, args)?;
    if matches.is_empty() {
        anyhow::bail!("No matching actions found");
    }

    let mut add_tags = normalize_tag_list(args.tag.clone());
    let mut remove_tags = normalize_tag_list(args.untag.clone());
    let mut mark_done = args.done;
    if let Some(priority) = parse_priority_value(args.priority.as_deref()) {
        remove_tags.push("@priority".to_string());
        add_tags.push(format!("@priority({priority})"));
    }
    if let Some(started) = args.started.as_deref() {
        remove_tags.push("@started".to_string());
        remove_tags.push("@start".to_string());
        add_tags.push(format!("@started({started})"));
    }
    if let Some(done_at) = args.end.as_deref() {
        remove_tags.push("@done".to_string());
        add_tags.push(format!("@done({done_at})"));
        mark_done = false;
    }
    if let Some(duration) = args.duration.as_deref() {
        remove_tags.push("@duration".to_string());
        add_tags.push(format!("@duration({duration})"));
    }

    let mut menu_archive = false;
    let mut menu_edit = false;
    let mut menu_restore = false;
    let mut menu_delete = false;
    let mut menu_note_extra: Vec<String> = Vec::new();
    let mut menu_overwrite_notes = false;
    let mut menu_move: Option<String> = None;
    let mut menu_priority: Option<u8> = None;
    let mut menu_plugin: Option<Plugin> = None;

    let needs_action_menu = args.plugin.is_none()
        && args.query.is_none()
        && args.search.is_empty()
        && add_tags.is_empty()
        && remove_tags.is_empty()
        && !mark_done
        && args.replace.is_none()
        && args.to.is_none()
        && !args.delete
        && !args.restore
        && !args.edit
        && args.note.is_empty();

    let selected = if needs_action_menu {
        let menu_plugins: Vec<Plugin> = PluginRegistry::default_dir()
            .and_then(|dir| PluginRegistry::discover(&dir).map(|r| r.plugins))
            .unwrap_or_default();
        let chosen = choose_actions_interactive(&matches, interactive)?;
        let op = prompt_update_operation(interactive, &menu_plugins)?;
        if op.cancelled {
            anyhow::bail!("Update cancelled");
        }
        menu_archive = op.archive;
        menu_edit = op.edit;
        menu_restore = op.restore;
        menu_delete = op.delete;
        menu_note_extra = op.note_lines;
        menu_overwrite_notes = op.overwrite_notes;
        menu_move = op.move_to_project;
        menu_priority = op.priority_level;
        add_tags = op.add_tags;
        remove_tags = op.remove_tags;
        mark_done = op.done;
        menu_plugin = op.plugin;
        chosen
    } else if args.all || !interactive {
        matches
    } else {
        choose_actions_interactive(&matches, interactive)?
    };

    if let Some(p) = menu_priority {
        remove_tags.push("@priority".to_string());
        add_tags.push(format!("@priority({p})"));
    }
    if menu_restore {
        remove_tags.push("@done".to_string());
    }

    let note_lines: Vec<String> = args
        .note
        .iter()
        .cloned()
        .chain(menu_note_extra.into_iter())
        .collect();
    let overwrite_notes = args.overwrite_notes || menu_overwrite_notes;
    let move_to_project = menu_move.or_else(|| args.to.clone());
    let delete_flag = args.delete || menu_delete;
    let restore_flag = args.restore || menu_restore;
    let effective_edit = args.edit || menu_edit;

    let selected_for_edit = if effective_edit {
        selected.clone()
    } else {
        Vec::new()
    };

    if let Some(plugin) = resolve_update_plugin(args)? {
        let total = persist_plugin_update(
            &mut files,
            &selected,
            &plugin,
            args.input.as_deref().and_then(PluginDataFormat::parse),
            args.output.as_deref().and_then(PluginDataFormat::parse),
            args.divider.as_deref(),
        )?;
        println!("Updated {total} action(s) via plugin {}", plugin.name);
        return Ok(());
    }

    if let Some(plugin) = menu_plugin {
        let total = persist_plugin_update(
            &mut files,
            &selected,
            &plugin,
            None,
            None,
            None,
        )?;
        println!("Updated {total} action(s) via plugin {}", plugin.name);
        return Ok(());
    }

    let mut by_file: HashMap<String, std::collections::HashSet<usize>> = HashMap::new();
    for action in selected {
        by_file
            .entry(action.source_file.clone())
            .or_default()
            .insert(action.line_index);
    }

    if menu_archive {
        let mut moved_count = 0usize;
        for todo in &mut files {
            let key = todo.path.display().to_string();
            if let Some(lines) = by_file.get(&key) {
                moved_count += todo.archive_actions_by_lines_with_options(
                    lines,
                    &note_lines,
                    overwrite_notes,
                )?;
            }
        }
        println!("Archived {moved_count} action(s)");
        return Ok(());
    }

    let base_mutation = UpdateMutation {
        add_tags: add_tags.clone(),
        remove_tags: remove_tags.clone(),
        done: mark_done,
        replace_text: args.replace.clone(),
        move_to_project,
        delete: delete_flag,
        restore: restore_flag,
        note_lines: note_lines.clone(),
        overwrite_notes,
        append_to_project_end: add_position_is_append(Some(effective_add_at(
            cli,
            args.at.as_deref(),
        ))),
    };

    let edit_map = if effective_edit {
        Some(run_multi_action_editor(
            args.editor.as_deref(),
            &selected_for_edit,
        )?)
    } else {
        None
    };

    let mut updated_count = 0usize;
    if effective_edit {
        let map = edit_map.as_ref().expect("edit map");
        for todo in &mut files {
            let key = todo.path.display().to_string();
            let Some(lines) = by_file.get(&key) else {
                continue;
            };
            let mut indices: Vec<usize> = lines.iter().copied().collect();
            indices.sort_by(|a, b| b.cmp(a));
            for line_idx in indices {
                let action = selected_for_edit
                    .iter()
                    .find(|a| {
                        a.line_index == line_idx
                            && update_paths_equivalent(Path::new(&a.source_file), &todo.path)
                    })
                    .with_context(|| {
                        format!(
                            "internal error: no selected action for {:?} line {}",
                            todo.path, line_idx
                        )
                    })?;
                let (new_text, new_notes) =
                    lookup_multi_action_edit(map, action).with_context(|| {
                        format!(
                            "missing edited content for {}:{} (keep `# ------` marker lines intact)",
                            action.source_file, action.line_index
                        )
                    })?;
                if new_text.trim().is_empty() {
                    anyhow::bail!(
                        "Edited action text cannot be empty ({})",
                        action.source_file
                    );
                }
                let mut m = base_mutation.clone();
                m.replace_text = Some(expand_date_tags_in_line(&new_text));
                m.note_lines = new_notes;
                m.overwrite_notes = true;
                updated_count += todo.apply_mutation_by_lines(&HashSet::from([line_idx]), &m)?;
            }
        }
    } else {
        for todo in &mut files {
            let key = todo.path.display().to_string();
            if let Some(lines) = by_file.get(&key) {
                updated_count += todo.apply_mutation_by_lines(lines, &base_mutation)?;
            }
        }
    }
    println!("Updated {updated_count} action(s)");
    Ok(())
}

/// Ruby `NA::Editor.default_editor`: env chain, then runnable check, then `which` fallback list.
fn resolve_update_editor(cli_override: Option<&str>) -> Result<String> {
    const FALLBACK: &[&str] = &["vim", "vi", "code", "subl", "mate", "mvim", "nano", "emacs"];

    if let Some(s) = cli_override {
        let t = s.trim();
        anyhow::ensure!(!t.is_empty(), "`--editor` is empty");
        if editor_first_token_executable(t) {
            return Ok(t.to_string());
        }
        anyhow::bail!(
            "editor from `--editor` is not runnable on this system: {:?}",
            t
        );
    }

    let env_specs = [
        std::env::var("NA_EDITOR").ok(),
        std::env::var("GIT_EDITOR").ok(),
        std::env::var("EDITOR").ok(),
    ];
    for spec in env_specs.into_iter().flatten() {
        let t = spec.trim();
        if t.is_empty() {
            continue;
        }
        if editor_first_token_executable(&spec) {
            return Ok(spec);
        }
    }

    for name in FALLBACK {
        if let Some(path) = which_executable_under_path(name) {
            return Ok(path);
        }
    }

    anyhow::bail!(
        "No runnable editor found. Install one of {}, or set NA_EDITOR, GIT_EDITOR, or EDITOR.",
        FALLBACK.join(", ")
    );
}

#[cfg(unix)]
fn path_entry_is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| !m.is_dir() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn path_entry_is_executable(path: &Path) -> bool {
    path.is_file()
}

fn executable_path_if_ok(path: &Path) -> Option<String> {
    if path_entry_is_executable(path) {
        return path.to_str().map(std::borrow::ToOwned::to_owned);
    }
    None
}

fn which_executable_under_path(prog_first_token: &str) -> Option<String> {
    let token = prog_first_token.trim();
    if token.is_empty() {
        return None;
    }
    let p = Path::new(token);
    if token.contains('/') {
        return executable_path_if_ok(p);
    }
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let full = dir.join(token);
        if let Some(hit) = executable_path_if_ok(&full) {
            return Some(hit);
        }
    }
    None
}

/// True when the leading token of `editor_spec` resolves to an executable path.
fn editor_first_token_executable(editor_spec: &str) -> bool {
    editor_spec
        .trim()
        .split_whitespace()
        .next()
        .is_some_and(|prog| !prog.is_empty() && which_executable_under_path(prog).is_some())
}

/// Ruby `NA::Editor.args_for_editor`: extra flags for GUI editors / vim when the spec is a bare binary name.
pub(crate) fn editor_program_and_extra_args(editor_spec: &str) -> (String, Vec<String>) {
    let parts: Vec<&str> = editor_spec.trim().split_whitespace().collect();
    if parts.is_empty() {
        return ("vi".to_string(), Vec::new());
    }
    if parts.len() >= 2 {
        return (
            parts[0].to_string(),
            parts[1..].iter().map(|s| s.to_string()).collect(),
        );
    }
    let prog = parts[0].to_string();
    let base = Path::new(&prog)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(prog.as_str());
    let flags: Vec<String> = match base {
        "vim" | "mvim" => vec!["-f".into()],
        "code" | "subl" | "mate" => vec!["-w".into()],
        _ => vec![],
    };
    (prog, flags)
}

fn run_editor_wait(path: &Path, editor_spec: &str) -> Result<()> {
    let (prog, flags) = editor_program_and_extra_args(editor_spec);
    let status = std::process::Command::new(&prog)
        .args(&flags)
        .arg(path)
        .status()?;
    if !status.success() {
        anyhow::bail!("Editor exited with failure status");
    }
    Ok(())
}

/// Ruby `NA::Editor.format_multi_action_input` (subset: instructions + `# ------ path:line` blocks).
fn format_multi_action_edit_buffer(actions: &[Action]) -> String {
    let header = concat!(
        "# Instructions:\n",
        "# - Edit the action text (the lines WITHOUT # comment markers)\n",
        "# - DO NOT remove or edit the lines starting with \"# ------\"\n",
        "# - Add notes on new lines after the action\n",
        "# - Blank lines are ignored\n",
        "# - Natural-language phrases inside @due(...), @started(...), @done(...), etc.\n",
        "#   are expanded to ISO timestamps when saved (Ruby-compatible).\n",
        "#\n",
    );
    let mut content = String::from(header);
    for action in actions {
        content.push_str(&format!(
            "# ------ {}:{}\n",
            action.source_file, action.line_index
        ));
        content.push_str(&action.text);
        content.push('\n');
        if !action.notes.is_empty() {
            for n in &action.notes {
                content.push_str(n);
                content.push('\n');
            }
        }
        content.push('\n');
    }
    content
}

/// Ruby `NA::Editor.parse_multi_action_output`.
pub(crate) fn parse_multi_action_edit_output(
    content: &str,
) -> Result<HashMap<(String, usize), (String, Vec<String>)>> {
    let mut results: HashMap<(String, usize), (String, Vec<String>)> = HashMap::new();
    let mut cur_key: Option<(String, usize)> = None;
    let mut cur_action: Option<String> = None;
    let mut cur_notes: Vec<String> = Vec::new();

    for line in content.lines() {
        let stripped = line.trim();
        if let Some(rest) = stripped.strip_prefix("# ------ ") {
            if let (Some(k), Some(a)) = (cur_key.take(), cur_action.take()) {
                results.insert(k, (a, std::mem::take(&mut cur_notes)));
            } else {
                cur_notes.clear();
            }
            let colon = rest.rfind(':').with_context(|| {
                format!("invalid `# ------` marker (expected path:line): {stripped:?}")
            })?;
            let path_part = rest[..colon].trim();
            let line_num = rest[colon + 1..].trim().parse::<usize>().with_context(|| {
                format!("invalid line number in `# ------` marker: {stripped:?}")
            })?;
            cur_key = Some((path_part.to_string(), line_num));
            cur_action = None;
            continue;
        }
        if stripped.starts_with('#') {
            continue;
        }
        if stripped.is_empty() {
            continue;
        }
        if cur_key.is_none() {
            continue;
        }
        if cur_action.is_none() {
            cur_action = Some(stripped.to_string());
        } else {
            cur_notes.push(stripped.to_string());
        }
    }
    if let (Some(k), Some(a)) = (cur_key, cur_action) {
        results.insert(k, (a, cur_notes));
    }
    Ok(results)
}

fn lookup_multi_action_edit(
    map: &HashMap<(String, usize), (String, Vec<String>)>,
    action: &Action,
) -> Option<(String, Vec<String>)> {
    let key = (action.source_file.clone(), action.line_index);
    if let Some(v) = map.get(&key) {
        return Some(v.clone());
    }
    for ((p, li), v) in map {
        if *li == action.line_index
            && update_paths_equivalent(Path::new(p), Path::new(&action.source_file))
        {
            return Some(v.clone());
        }
    }
    None
}

fn run_multi_action_editor(
    editor_override: Option<&str>,
    actions: &[Action],
) -> Result<HashMap<(String, usize), (String, Vec<String>)>> {
    if actions.is_empty() {
        anyhow::bail!("No actions to edit");
    }
    let body = format_multi_action_edit_buffer(actions);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "na_update_edit_{}.na",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    fs::write(&path, body)?;
    let editor = resolve_update_editor(editor_override)?;
    run_editor_wait(&path, &editor)?;
    let edited = fs::read_to_string(&path)?;
    let _ = fs::remove_file(&path);
    parse_multi_action_edit_output(&edited)
}

fn run_edit(cli: &Cli, args: &EditArgs) -> Result<()> {
    let mut update_args = UpdateArgs {
        query: args.query.clone(),
        tag: Vec::new(),
        untag: Vec::new(),
        done: false,
        file: args.file.clone(),
        depth: args.depth,
        in_todo: args.in_todo.clone(),
        search: args.search.clone(),
        all: args.all,
        replace: Some(args.text.clone()),
        to: None,
        project: None,
        tagged: Vec::new(),
        regex: false,
        exact: false,
        search_notes: true,
        no_search_notes: false,
        priority: None,
        at: None,
        archive: false,
        edit: false,
        editor: None,
        delete: false,
        restore: false,
        note: Vec::new(),
        overwrite_notes: false,
        started: None,
        end: None,
        duration: None,
        plugin: None,
        input: None,
        output: None,
        divider: None,
    };
    if update_args.query.is_none() && update_args.search.is_empty() {
        update_args.all = true;
    }
    run_update(cli, &update_args)
}

fn run_restore(cli: &Cli, args: &UpdateArgs) -> Result<()> {
    let mut restore = args.clone();
    restore.restore = true;
    restore.untag.push("@done".to_string());
    run_update(cli, &restore)
}

fn run_move(cli: &Cli, args: &MoveArgs) -> Result<()> {
    let update = UpdateArgs {
        query: args.query.clone(),
        tag: Vec::new(),
        untag: Vec::new(),
        done: false,
        file: args.file.clone(),
        depth: args.depth,
        in_todo: args.in_todo.clone(),
        search: args.search.clone(),
        all: args.all,
        replace: None,
        to: Some(args.to.clone()),
        project: args.from.clone(),
        tagged: args.tagged.clone(),
        regex: args.regex,
        exact: args.exact,
        search_notes: args.search_notes,
        no_search_notes: !args.search_notes,
        priority: None,
        at: args.at.clone(),
        archive: false,
        edit: false,
        editor: None,
        delete: false,
        restore: false,
        note: Vec::new(),
        overwrite_notes: false,
        started: None,
        end: None,
        duration: None,
        plugin: None,
        input: None,
        output: None,
        divider: None,
    };
    run_update(cli, &update)
}

fn run_tag(cli: &Cli, args: &TagArgs) -> Result<()> {
    let mut add = Vec::new();
    let mut remove = Vec::new();
    for raw in &args.tags {
        for t in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            if let Some(stripped) = t.strip_prefix('!').or_else(|| t.strip_prefix('-')) {
                remove.push(if stripped.starts_with('@') {
                    stripped.to_string()
                } else {
                    format!("@{stripped}")
                });
            } else {
                add.push(if t.starts_with('@') {
                    t.to_string()
                } else {
                    format!("@{t}")
                });
            }
        }
    }
    let mut tagged = args.tagged.clone();
    if args.done {
        tagged.push("@done".to_string());
    }
    let update = UpdateArgs {
        query: args.query.clone(),
        tag: add,
        untag: remove,
        done: false,
        file: args.file.clone(),
        depth: args.depth,
        in_todo: args.in_todo.clone(),
        search: args.search.clone(),
        all: args.all,
        replace: None,
        to: None,
        project: None,
        tagged,
        regex: args.regex,
        exact: args.exact,
        search_notes: args.search_notes,
        no_search_notes: !args.search_notes,
        priority: None,
        at: None,
        archive: false,
        edit: false,
        editor: None,
        delete: false,
        restore: false,
        note: Vec::new(),
        overwrite_notes: false,
        started: None,
        end: None,
        duration: None,
        plugin: None,
        input: None,
        output: None,
        divider: None,
    };
    run_update(cli, &update)
}

fn run_open(cli: &Cli, args: &OpenArgs) -> Result<()> {
    let mut paths = if let Some(path) = &cli.global_file {
        vec![path.clone()]
    } else {
        discover_taskpaper_files_with_options(
            &cli.extension,
            effective_discovery_depth_usize(cli, args.depth, 1),
            false,
        )?
    };
    if !args.in_todo.is_empty() {
        let specs = parse_todo_specs(&args.in_todo);
        paths.retain(|p| match_todo_path(p.to_string_lossy().as_ref(), &specs));
    }
    let Some(path) = paths.first() else {
        anyhow::bail!("No todo file found");
    };
    let (program, program_args) =
        open_command_for_target(path, args.editor.as_deref(), args.app.as_deref());
    let status = std::process::Command::new(program)
        .args(program_args)
        .status()?;
    if !status.success() {
        anyhow::bail!("Open command failed");
    }
    Ok(())
}

fn open_command_for_target(
    path: &Path,
    editor_override: Option<&str>,
    app_override: Option<&str>,
) -> (String, Vec<String>) {
    if let Some(app) = app_override {
        #[cfg(target_os = "macos")]
        {
            return (
                "open".to_string(),
                vec![
                    "-a".to_string(),
                    app.to_string(),
                    path.to_string_lossy().to_string(),
                ],
            );
        }
        #[cfg(not(target_os = "macos"))]
        {
            return (app.to_string(), vec![path.to_string_lossy().to_string()]);
        }
    }
    let editor = editor_override
        .map(ToString::to_string)
        .or_else(|| std::env::var("EDITOR").ok())
        .unwrap_or_else(|| "vi".to_string());
    (editor, vec![path.to_string_lossy().to_string()])
}

fn run_projects(cli: &Cli, args: &ProjectsArgs) -> Result<()> {
    let files = if let Some(path) = &cli.global_file {
        vec![TodoFile::load(path)?]
    } else {
        discover_taskpaper_files_with_options(
            &cli.extension,
            effective_discovery_depth_usize(cli, args.depth, 1),
            false,
        )?
            .iter()
            .map(|p| TodoFile::load(p))
            .collect::<Result<Vec<_>, _>>()?
    };
    for file in files {
        for project in file.project_paths() {
            if args.paths {
                println!("{} :: {}", file.path.display(), project);
            } else {
                println!("{project}");
            }
        }
    }
    Ok(())
}

fn run_todos(cli: &Cli, args: &TodosArgs) -> Result<()> {
    let paths = if let Some(path) = &cli.global_file {
        vec![path.clone()]
    } else {
        discover_taskpaper_files(&cli.extension)?
    };
    for p in &paths {
        println!("{}", p.display());
    }
    if args.edit {
        let Some(first) = paths.first() else {
            return Ok(());
        };
        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
        let _ = std::process::Command::new(editor).arg(first).status()?;
    }
    Ok(())
}

fn run_undo(cli: &Cli, args: &UndoArgs) -> Result<()> {
    let mut backup_files: Vec<PathBuf> = Vec::new();
    if let Some(path) = &cli.global_file {
        let target = crate::io::fs::backup_path(path);
        if target.exists() {
            backup_files.push(target);
        }
    } else {
        for entry in walkdir::WalkDir::new(na_backup_dir()).max_depth(10) {
            let entry = entry?;
            if entry.file_type().is_file() {
                let p = entry.path();
                if p.extension().and_then(|s| s.to_str()) == Some("bak") {
                    backup_files.push(p.to_path_buf());
                }
            }
        }
    }
    if backup_files.is_empty() {
        anyhow::bail!("No backup files found");
    }
    backup_files.sort_by(|a, b| {
        let ma = fs::metadata(a)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        let mb = fs::metadata(b)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        mb.cmp(&ma).then_with(|| a.cmp(b))
    });
    let pick = if args.select {
        let options: Vec<String> = backup_files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
        let idx = choose_from_menu("Select backup to restore", &options, interactive)?;
        backup_files
            .get(idx)
            .cloned()
            .context("No backup candidate")?
    } else {
        backup_files
            .first()
            .cloned()
            .context("No backup candidate")?
    };
    let target = restore_target_from_backup_path(&pick);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&pick, &target)
        .with_context(|| format!("Failed restoring {:?} to {:?}", pick, target))?;
    println!("Restored {}", target.display());
    Ok(())
}

fn restore_target_from_backup_path(backup: &Path) -> PathBuf {
    let rel = backup
        .strip_prefix(na_backup_dir())
        .unwrap_or(backup)
        .to_path_buf();
    let mut target = PathBuf::from("/");
    target.push(rel);
    let target_name = target
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("todo.taskpaper.bak")
        .trim_end_matches(".bak")
        .to_string();
    target.set_file_name(target_name);
    target
}

fn run_scan(cli: &Cli, args: &ScanArgs) -> Result<()> {
    let files = discover_taskpaper_files_with_options(
        &cli.extension,
        effective_discovery_depth_usize(cli, args.depth, 1),
        args.hidden,
    )?
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    let discovered: HashSet<String> = files.iter().cloned().collect();
    let existing = read_scan_registry()?;
    let mut to_add: Vec<String> = discovered.difference(&existing).cloned().collect();
    let mut to_remove: Vec<String> = existing.difference(&discovered).cloned().collect();
    to_add.sort();
    to_remove.sort();

    for f in &files {
        println!("{f}");
    }
    if args.prune || args.dry_run {
        for p in &to_add {
            println!("+ {p}");
        }
        for p in &to_remove {
            println!("- {p}");
        }
        println!("scan changes: +{} -{}", to_add.len(), to_remove.len());
    }
    if !args.dry_run {
        let next = if args.prune {
            discovered
        } else {
            discovered.union(&existing).cloned().collect()
        };
        write_scan_registry(&next)?;
    }
    Ok(())
}

fn scan_registry_path() -> PathBuf {
    na_data_dir().join("scan_paths.txt")
}

fn read_scan_registry() -> Result<HashSet<String>> {
    let path = scan_registry_path();
    if !path.exists() {
        return Ok(HashSet::new());
    }
    let content = fs::read_to_string(path)?;
    Ok(content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect())
}

fn write_scan_registry(paths: &HashSet<String>) -> Result<()> {
    let path = scan_registry_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut sorted: Vec<String> = paths.iter().cloned().collect();
    sorted.sort();
    fs::write(path, sorted.join("\n") + "\n")?;
    Ok(())
}

fn run_init(_cli: &Cli) -> Result<()> {
    let path = PathBuf::from("todo.taskpaper");
    if path.exists() {
        anyhow::bail!("todo.taskpaper already exists");
    }
    fs::write(&path, "Inbox:\n").with_context(|| format!("Failed writing {:?}", path))?;
    println!("Created {}", path.display());
    Ok(())
}

fn run_initconfig(cli: &Cli, args: &InitConfigArgs) -> Result<()> {
    let path = find_na_rc_path();
    if path.is_file() && !args.force {
        anyhow::bail!(
            "Config file already exists at {}. Use --force to overwrite.",
            path.display()
        );
    }
    let globals = rc_globals_from_cli(cli);
    write_na_rc(&path, &globals)?;
    println!("Wrote config to {}", path.display());
    Ok(())
}

fn run_prompt(cli: &Cli, args: &PromptArgs) -> Result<()> {
    match args.command.clone().unwrap_or(PromptCommands::Show) {
        PromptCommands::Show => {
            let hook = prompt_hook_script(cli)?;
            let shell = std::env::var("SHELL").unwrap_or_default();
            let profile = if shell.contains("fish") {
                "~/.config/fish/conf.d/na.fish"
            } else if shell.contains("zsh") {
                "~/.zshrc"
            } else {
                "~/.bash_profile"
            };
            println!("# Add this to {profile}");
            print!("{hook}");
            Ok(())
        }
        PromptCommands::Install => {
            if cli.global_file.is_some() && parse_cwd_as(&cli.cwd_as) == CwdAsMode::None {
                anyhow::bail!(
                    "When using a global file, a prompt hook requires `--cwd_as [tag|project]`"
                );
            }
            let profile = prompt_profile_path()?;
            let hook = prompt_hook_script(cli)?;
            let mut content = if profile.exists() {
                fs::read_to_string(&profile)?
            } else {
                String::new()
            };
            if !content.contains("prompt hook for na") && !content.contains("Prompt Command") {
                if !content.is_empty() && !content.ends_with('\n') {
                    content.push('\n');
                }
                content.push_str(&hook);
                if let Some(parent) = profile.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&profile, content)?;
                println!("Installed prompt hook in {}", profile.display());
            } else {
                println!("Prompt hook already present in {}", profile.display());
            }
            Ok(())
        }
    }
}

fn run_changes() -> Result<()> {
    let changelog = PathBuf::from("CHANGELOG.md");
    if !changelog.exists() {
        anyhow::bail!("CHANGELOG.md not found");
    }
    print!("{}", fs::read_to_string(changelog)?);
    Ok(())
}

fn prompt_profile_path() -> Result<PathBuf> {
    if let Ok(override_path) = std::env::var("NA_PROMPT_PROFILE") {
        return Ok(PathBuf::from(override_path));
    }
    let home = std::env::var("HOME").context("HOME is not set")?;
    let shell = std::env::var("SHELL").unwrap_or_default();
    let profile = if shell.contains("zsh") {
        ".zshrc"
    } else if shell.contains("bash") {
        ".bashrc"
    } else {
        ".profile"
    };
    Ok(PathBuf::from(home).join(profile))
}

fn run_archive(cli: &Cli, args: &ArchiveArgs) -> Result<()> {
    let mut files = load_archive_todo_files(cli, args)?;
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let matches = collect_archive_candidates(&files, args)?;
    if matches.is_empty() {
        anyhow::bail!("No matching actions found");
    }
    let selected = if args.all || !interactive {
        matches
    } else {
        choose_actions_interactive(&matches, interactive)?
    };

    let mut by_file: HashMap<String, std::collections::HashSet<usize>> = HashMap::new();
    for action in selected {
        by_file
            .entry(action.source_file.clone())
            .or_default()
            .insert(action.line_index);
    }
    let mut moved_count = 0usize;
    for todo in &mut files {
        let key = todo.path.display().to_string();
        if let Some(lines) = by_file.get(&key) {
            moved_count += todo.archive_actions_by_lines_with_options(
                lines,
                &args.note,
                args.overwrite_notes,
            )?;
        }
    }
    println!("Archived {moved_count} action(s)");
    Ok(())
}

fn load_archive_todo_files(cli: &Cli, args: &ArchiveArgs) -> Result<Vec<TodoFile>> {
    if let Some(path) = &args.file {
        return TodoFile::load(path)
            .with_context(|| format!("Failed to read {:?}", path))
            .map(|todo| vec![todo]);
    }
    if let Some(path) = &cli.global_file {
        return TodoFile::load(path)
            .with_context(|| format!("Failed to read {:?}", path))
            .map(|todo| vec![todo]);
    }
    let mut paths = discover_taskpaper_files_with_options(
        &cli.extension,
        effective_discovery_depth_usize(cli, args.depth, 1),
        false,
    )?;
    if !args.in_todo.is_empty() {
        let specs = parse_todo_specs(&args.in_todo);
        paths.retain(|path| match_todo_path(path.to_string_lossy().as_ref(), &specs));
    }
    paths
        .iter()
        .map(|path| TodoFile::load(path).with_context(|| format!("Failed to read {:?}", path)))
        .collect()
}

fn collect_archive_candidates(files: &[TodoFile], args: &ArchiveArgs) -> Result<Vec<Action>> {
    let mut matches = if let Some(q) = args.query.as_deref() {
        let query = Query::parse(q)?;
        evaluate_query(files, &query)
    } else if !args.search.is_empty() {
        let query = Query::parse(&args.search.join(" "))?;
        evaluate_query(files, &query)
    } else {
        files.iter().flat_map(TodoFile::actions).collect()
    };

    if let Some(project) = &args.project {
        let needle = project.to_ascii_lowercase();
        matches.retain(|a| {
            a.project_chain
                .iter()
                .any(|p| p.to_ascii_lowercase().contains(&needle))
        });
    }
    if !args.tagged.is_empty() {
        matches.retain(|a| {
            args.tagged.iter().all(|t| {
                let tag = if t.starts_with('@') {
                    t.to_ascii_lowercase()
                } else {
                    format!("@{}", t.to_ascii_lowercase())
                };
                a.tags.iter().any(|x| x.eq_ignore_ascii_case(&tag))
            })
        });
    }
    if args.done {
        matches.retain(|a| a.done);
    } else {
        matches.retain(|a| !a.done);
    }
    Ok(matches)
}

fn load_update_todo_files(cli: &Cli, args: &UpdateArgs) -> Result<Vec<TodoFile>> {
    if let Some(path) = &args.file {
        return TodoFile::load(path)
            .with_context(|| format!("Failed to read {:?}", path))
            .map(|todo| vec![todo]);
    }
    if let Some(path) = &cli.global_file {
        return TodoFile::load(path)
            .with_context(|| format!("Failed to read {:?}", path))
            .map(|todo| vec![todo]);
    }
    let mut paths = discover_taskpaper_files_with_options(
        &cli.extension,
        effective_discovery_depth_usize(cli, args.depth, 1),
        false,
    )?;
    if !args.in_todo.is_empty() {
        let specs = parse_todo_specs(&args.in_todo);
        paths.retain(|path| match_todo_path(path.to_string_lossy().as_ref(), &specs));
    }
    paths
        .iter()
        .map(|path| TodoFile::load(path).with_context(|| format!("Failed to read {:?}", path)))
        .collect()
}

fn update_path_for_compare(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Match a loaded todo path against a query path (`foo.taskpaper`, `./a/foo.taskpaper`, or absolute).
fn update_paths_equivalent(loaded: &Path, query: &Path) -> bool {
    if loaded == query {
        return true;
    }
    let ls = update_path_for_compare(loaded)
        .trim_start_matches("./")
        .to_string();
    let qs = update_path_for_compare(query)
        .trim_start_matches("./")
        .to_string();
    if ls == qs {
        return true;
    }
    if let (Ok(lc), Ok(qc)) = (loaded.canonicalize(), query.canonicalize()) {
        if lc == qc {
            return true;
        }
    }
    if qs.contains('/') {
        ls.ends_with(qs.as_str()) || ls.ends_with(&format!("/{qs}"))
    } else {
        loaded.file_name().and_then(|n| n.to_str()) == Some(qs.as_str())
    }
}

/// Trailing `:LINE` decimal suffix; path may contain other `:` (e.g. `pro:ject/file.tp`).
/// Returns `None` so callers fall through to substring / `@search` / `--regex`.
fn parse_update_path_colon_line_query(query: &str) -> Option<(String, usize)> {
    let q = query.trim();
    if q.starts_with("@search(") {
        return None;
    }
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"(?ms)^(?P<p>.+):(?P<n>\d+)$").expect("update path:line regex")
    });
    let caps = re.captures(q)?;
    let path_str = caps.name("p")?.as_str().trim();
    if path_str.is_empty() {
        return None;
    }
    let line_1 = caps.name("n")?.as_str().parse::<usize>().ok()?;
    if line_1 == 0 {
        return None;
    }
    Some((path_str.to_string(), line_1))
}

/// `PATH:LINE` selection against loaded todos. **`None`** → not path:line syntax.
/// **`Some(vec)`** → syntax matched; **`vec` empty** if no todo path matched or nothing on that line.
fn seed_matches_update_path_colon_line(files: &[TodoFile], query: &str) -> Option<Vec<Action>> {
    let (path_str, line_1based) = parse_update_path_colon_line_query(query)?;
    let idx = line_1based - 1;
    let query_path = Path::new(path_str.trim());
    let mut saw_matching_file = false;
    let mut out = Vec::new();
    for tf in files {
        if update_paths_equivalent(&tf.path, query_path) {
            saw_matching_file = true;
            out.extend(tf.actions().into_iter().filter(|a| a.line_index == idx));
        }
    }
    if !saw_matching_file {
        return Some(Vec::new());
    }
    Some(out)
}

fn collect_update_candidates(files: &[TodoFile], args: &UpdateArgs) -> Result<Vec<Action>> {
    let mut out = if let Some(q) = args.query.as_deref() {
        update_seed_matches(files, q, args)?
    } else if !args.search.is_empty() {
        update_seed_matches(files, &args.search.join(" "), args)?
    } else {
        files.iter().flat_map(TodoFile::actions).collect()
    };
    if let Some(project) = &args.project {
        let needle = project.to_ascii_lowercase();
        out.retain(|a| {
            a.project_chain
                .iter()
                .any(|p| p.to_ascii_lowercase().contains(&needle))
        });
    }
    if !args.tagged.is_empty() {
        out.retain(|a| {
            args.tagged.iter().all(|t| {
                let tag = if t.starts_with('@') {
                    t.to_ascii_lowercase()
                } else {
                    format!("@{}", t.to_ascii_lowercase())
                };
                a.tags.iter().any(|x| x.eq_ignore_ascii_case(&tag))
            })
        });
    }
    Ok(out)
}

fn update_seed_matches(
    files: &[TodoFile],
    pattern: &str,
    args: &UpdateArgs,
) -> Result<Vec<Action>> {
    let query = pattern.trim();
    if query.starts_with("@search(") && !args.regex && !args.exact {
        let parsed = Query::parse(query)?;
        return Ok(evaluate_query(files, &parsed));
    }
    if let Some(found) = seed_matches_update_path_colon_line(files, query) {
        return Ok(found);
    }
    let mut out: Vec<Action> = files.iter().flat_map(TodoFile::actions).collect();
    let include_notes = args.search_notes && !args.no_search_notes;
    let haystack = |a: &Action| {
        if include_notes {
            format!("{} {}", a.text, a.notes.join(" "))
        } else {
            a.text.clone()
        }
    };
    if args.regex {
        let rx = regex::Regex::new(query)?;
        out.retain(|a| rx.is_match(&haystack(a)));
        return Ok(out);
    }
    let needle = query.to_ascii_lowercase();
    if args.exact {
        out.retain(|a| haystack(a).to_ascii_lowercase().contains(&needle));
        return Ok(out);
    }
    let parts: Vec<String> = needle
        .split_whitespace()
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect();
    if parts.is_empty() {
        return Ok(out);
    }
    out.retain(|a| {
        let h = haystack(a).to_ascii_lowercase();
        parts.iter().all(|p| h.contains(p))
    });
    Ok(out)
}

/// Interactive-only choices for `na update` when no mutation flags are given (Ruby gem parity).
#[derive(Debug, Default, Clone)]
struct UpdateMenuSelection {
    cancelled: bool,
    /// Run a discovered plugin on the selection and persist merged stdout (Ruby-style).
    plugin: Option<Plugin>,
    /// Move items to Archive project with @done (same as `na archive`).
    archive: bool,
    /// Open `$EDITOR` with multi-action buffer (`format_multi_action_edit_buffer`).
    edit: bool,
    /// Strip `@done` and move from Archive to Inbox where applicable.
    restore: bool,
    delete: bool,
    done: bool,
    add_tags: Vec<String>,
    remove_tags: Vec<String>,
    note_lines: Vec<String>,
    overwrite_notes: bool,
    move_to_project: Option<String>,
    priority_level: Option<u8>,
}

fn resolve_update_plugin(args: &UpdateArgs) -> Result<Option<Plugin>> {
    let Some(name) = args.plugin.as_deref() else {
        return Ok(None);
    };
    let registry = PluginRegistry::discover(&PluginRegistry::default_dir()?)?;
    Ok(Some(registry.resolve_plugin(name)?))
}

fn persist_plugin_update(
    files: &mut [TodoFile],
    selected: &[Action],
    plugin: &Plugin,
    input: Option<PluginDataFormat>,
    output: Option<PluginDataFormat>,
    divider: Option<&str>,
) -> Result<usize> {
    let runner = PluginRunner::new(plugin.clone());
    let stdout = runner.run_with_formats(selected, input, output, divider)?;
    let out_fmt = output.unwrap_or(plugin.output_format);
    apply_plugin_stdout_to_files(files, selected, &stdout, out_fmt, divider)
}

fn choose_actions_interactive(actions: &[Action], interactive: bool) -> Result<Vec<Action>> {
    if actions.is_empty() {
        anyhow::bail!("No matching actions found");
    }
    if !interactive {
        return Ok(actions.to_vec());
    }
    let labels: Vec<String> = actions
        .iter()
        .map(|a| {
            format!(
                "{}:{} | {}",
                Path::new(&a.source_file)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&a.source_file),
                a.line_index + 1,
                a.text
            )
        })
        .collect();
    let selected = MultiSelect::new(
        "Select actions to update (Space to toggle, Enter to confirm)",
        labels.clone(),
    )
    .prompt()
    .map_err(map_inquire_error)?;
    if selected.is_empty() {
        anyhow::bail!("No actions selected");
    }
    let selected_set: HashSet<String> = selected.into_iter().collect();
    Ok(actions
        .iter()
        .zip(labels.iter())
        .filter_map(|(a, label)| selected_set.contains(label).then_some(a.clone()))
        .collect())
}

#[cfg(test)]
fn parse_multi_selection(input: &str, len: usize) -> Option<Vec<usize>> {
    let s = input.trim().to_ascii_lowercase();
    if s == "all" {
        return Some((0..len).collect());
    }
    let mut out = std::collections::BTreeSet::new();
    for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        if let Some((a, b)) = part.split_once('-') {
            let start = a.trim().parse::<usize>().ok()?;
            let end = b.trim().parse::<usize>().ok()?;
            if start == 0 || end == 0 || start > end || end > len {
                return None;
            }
            for n in start..=end {
                out.insert(n - 1);
            }
        } else {
            let n = part.parse::<usize>().ok()?;
            if n == 0 || n > len {
                return None;
            }
            out.insert(n - 1);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out.into_iter().collect())
    }
}

fn prompt_update_operation(interactive: bool, plugins: &[Plugin]) -> Result<UpdateMenuSelection> {
    if !interactive {
        return Ok(UpdateMenuSelection::default());
    }
    // Order aligned with Ruby `na update` interactive menu (fzf/gum).
    let mut ops = vec![
        "Add Note".to_string(),
        "Archive".to_string(),
        "Restore".to_string(),
        "Move to Project".to_string(),
        "Set Priority".to_string(),
        "Edit".to_string(),
        "Finish (mark done)".to_string(),
        "Delete".to_string(),
        "Remove Tag".to_string(),
        "Add Tag".to_string(),
    ];
    for p in plugins {
        ops.push(format!("Plugin: {}", p.name));
    }
    ops.push("Cancel".to_string());
    let op = match Select::new("Choose update action", ops).prompt() {
        Ok(v) => v,
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => {
            return Ok(UpdateMenuSelection {
                cancelled: true,
                ..UpdateMenuSelection::default()
            })
        }
        Err(e) => return Err(map_inquire_error(e)),
    };
    match op.as_str() {
        "Add Note" => {
            let input = Text::new("Note text (paste multiple lines if needed)")
                .prompt()
                .map_err(map_inquire_error)?;
            let lines: Vec<String> = input
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect();
            if lines.is_empty() {
                anyhow::bail!("Empty note");
            }
            let overwrite = Confirm::new("Replace existing notes instead of appending?")
                .with_default(false)
                .prompt()
                .map_err(map_inquire_error)?;
            Ok(UpdateMenuSelection {
                note_lines: lines,
                overwrite_notes: overwrite,
                ..UpdateMenuSelection::default()
            })
        }
        "Archive" => Ok(UpdateMenuSelection {
            archive: true,
            ..UpdateMenuSelection::default()
        }),
        "Restore" => Ok(UpdateMenuSelection {
            restore: true,
            ..UpdateMenuSelection::default()
        }),
        "Move to Project" => {
            let project = Text::new("Target project (e.g. Inbox, Work, or Work:Sub)")
                .prompt()
                .map_err(map_inquire_error)?;
            let t = project.trim();
            if t.is_empty() {
                anyhow::bail!("Project name required");
            }
            Ok(UpdateMenuSelection {
                move_to_project: Some(t.to_string()),
                ..UpdateMenuSelection::default()
            })
        }
        "Set Priority" => {
            let input = Text::new("Priority: 1-5, or h / m / l")
                .prompt()
                .map_err(map_inquire_error)?;
            let p = parse_priority_value(Some(&input))
                .ok_or_else(|| anyhow::anyhow!("Invalid priority (use 1-5 or h, m, l)"))?;
            Ok(UpdateMenuSelection {
                priority_level: Some(p),
                ..UpdateMenuSelection::default()
            })
        }
        "Edit" => Ok(UpdateMenuSelection {
            edit: true,
            ..UpdateMenuSelection::default()
        }),
        "Finish (mark done)" => Ok(UpdateMenuSelection {
            done: true,
            ..UpdateMenuSelection::default()
        }),
        "Delete" => {
            let ok = Confirm::new("Delete selected action(s)?")
                .with_default(false)
                .prompt()
                .map_err(map_inquire_error)?;
            if !ok {
                return Ok(UpdateMenuSelection {
                    cancelled: true,
                    ..UpdateMenuSelection::default()
                });
            }
            Ok(UpdateMenuSelection {
                delete: true,
                ..UpdateMenuSelection::default()
            })
        }
        "Remove Tag" => {
            let input = Text::new("Tag(s) to remove (comma-separated)")
                .prompt()
                .map_err(map_inquire_error)?;
            Ok(UpdateMenuSelection {
                remove_tags: parse_tag_input(&input),
                ..UpdateMenuSelection::default()
            })
        }
        "Add Tag" => {
            let input = Text::new("Tag(s) to add (comma-separated)")
                .prompt()
                .map_err(map_inquire_error)?;
            Ok(UpdateMenuSelection {
                add_tags: parse_tag_input(&input),
                ..UpdateMenuSelection::default()
            })
        }
        "Cancel" => Ok(UpdateMenuSelection {
            cancelled: true,
            ..UpdateMenuSelection::default()
        }),
        other if other.starts_with("Plugin: ") => {
            let name = other
                .strip_prefix("Plugin: ")
                .unwrap_or("")
                .trim();
            anyhow::ensure!(!name.is_empty(), "Empty plugin name");
            let plugin = plugins
                .iter()
                .find(|p| p.name == name)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Unknown plugin: {name}"))?;
            Ok(UpdateMenuSelection {
                plugin: Some(plugin),
                ..UpdateMenuSelection::default()
            })
        }
        _ => anyhow::bail!("Invalid menu choice"),
    }
}

fn map_inquire_error(err: InquireError) -> anyhow::Error {
    match err {
        InquireError::OperationCanceled | InquireError::OperationInterrupted => {
            anyhow::anyhow!("Selection cancelled")
        }
        other => anyhow::anyhow!(other.to_string()),
    }
}

fn parse_tag_input(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(normalize_tag_token)
        .collect()
}

fn normalize_tag_token(raw: &str) -> String {
    let s = raw.trim();
    if s.starts_with('@') {
        s.to_string()
    } else {
        format!("@{s}")
    }
}

fn normalize_tag_list(tags: Vec<String>) -> Vec<String> {
    tags.into_iter()
        .flat_map(|t| parse_tag_input(&t))
        .collect()
}

fn run_saved(command: &SavedCommands) -> Result<()> {
    match command {
        SavedCommands::List => {
            for name in list_saved_searches()? {
                println!("{name}");
            }
            Ok(())
        }
        SavedCommands::Run { title } => {
            let path = saved_search_path(title)?;
            if !path.exists() {
                anyhow::bail!("Saved search not found: {title}");
            }
            let script = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read saved search {:?}", path))?;
            let command = script.trim();
            if command.is_empty() {
                anyhow::bail!("Saved search is empty: {}", path.display());
            }
            let status = std::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .status()
                .with_context(|| format!("Failed to execute saved search {title}"))?;
            if !status.success() {
                anyhow::bail!("Saved search exited with status {status}: {title}");
            }
            Ok(())
        }
        SavedCommands::Edit { title, editor } => {
            let path = saved_search_path(title)?;
            if !path.exists() {
                anyhow::bail!("Saved search not found: {title}");
            }
            let editor_cmd = editor
                .clone()
                .or_else(|| std::env::var("EDITOR").ok())
                .unwrap_or_else(|| "vi".to_string());
            let status = std::process::Command::new(&editor_cmd)
                .arg(&path)
                .status()
                .with_context(|| format!("Failed to launch editor {}", editor_cmd))?;
            if !status.success() {
                anyhow::bail!("Editor exited with status {status}");
            }
            Ok(())
        }
        SavedCommands::Delete { title } => {
            let path = saved_search_path(title)?;
            if !path.exists() {
                anyhow::bail!("Saved search not found: {title}");
            }
            fs::remove_file(&path).with_context(|| format!("Failed to remove {:?}", path))?;
            println!("Deleted saved search {title}");
            Ok(())
        }
        SavedCommands::Select => {
            let names = list_saved_searches()?;
            if names.is_empty() {
                anyhow::bail!("No saved searches found");
            }
            let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
            let idx = choose_from_menu("Select saved search to run", &names, interactive)?;
            run_saved(&SavedCommands::Run {
                title: names[idx].clone(),
            })
        }
    }
}

fn list_saved_searches() -> Result<Vec<String>> {
    let dir = saved_searches_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("Failed to read {:?}", dir))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("txt") {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            out.push(stem.to_string());
        }
    }
    out.sort();
    Ok(out)
}

fn saved_searches_dir() -> PathBuf {
    na_data_dir().join("searches")
}

fn saved_search_path(title: &str) -> Result<PathBuf> {
    let slug = saved_search_slug(title);
    if slug.is_empty() {
        anyhow::bail!("Invalid saved search title: {title}");
    }
    Ok(saved_searches_dir().join(format!("{slug}.txt")))
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
        PluginCommands::Run {
            plugin,
            query,
            input,
            output,
            divider,
            file,
            depth,
            in_todo,
            search,
            done,
            tagged,
        } => {
            let files = if let Some(path) = file {
                vec![TodoFile::load(path)?]
            } else if let Some(path) = &cli.global_file {
                vec![TodoFile::load(path)?]
            } else {
                let mut paths = discover_taskpaper_files_with_options(
                    &cli.extension,
                    effective_discovery_depth(cli, depth.clone(), 5),
                    false,
                )?;
                if !in_todo.is_empty() {
                    let specs = parse_todo_specs(in_todo);
                    paths.retain(|p| match_todo_path(p.to_string_lossy().as_ref(), &specs));
                }
                paths
                    .iter()
                    .map(|p| TodoFile::load(p))
                    .collect::<Result<Vec<_>, _>>()?
            };
            let q = Query::parse(query)?;
            let mut actions = evaluate_query(&files, &q);
            if !search.is_empty() {
                let needle = search.join(" ").to_ascii_lowercase();
                actions.retain(|a| a.text.to_ascii_lowercase().contains(&needle));
            }
            if !tagged.is_empty() {
                actions.retain(|a| {
                    tagged.iter().all(|t| {
                        let tag = if t.starts_with('@') {
                            t.to_ascii_lowercase()
                        } else {
                            format!("@{}", t.to_ascii_lowercase())
                        };
                        a.tags.iter().any(|x| x.eq_ignore_ascii_case(&tag))
                    })
                });
            }
            if !done {
                actions.retain(|a| !a.done);
            }
            let runner = registry.plugin(plugin)?;
            let output = runner.run_with_formats(
                &actions,
                input.as_deref().and_then(PluginDataFormat::parse),
                output.as_deref().and_then(PluginDataFormat::parse),
                divider.as_deref(),
            )?;
            println!("{output}");
            Ok(())
        }
        PluginCommands::Enable { plugin } => {
            let path = registry.plugin_path_from_name_or_path(plugin)?;
            PluginRegistry::set_enabled(&path, true)?;
            println!("Enabled plugin {}", path.display());
            Ok(())
        }
        PluginCommands::Disable { plugin } => {
            let path = registry.plugin_path_from_name_or_path(plugin)?;
            PluginRegistry::set_enabled(&path, false)?;
            println!("Disabled plugin {}", path.display());
            Ok(())
        }
        PluginCommands::New { plugin } => {
            let path = PluginRegistry::create_plugin_stub(plugin)?;
            println!("Created plugin {}", path.display());
            Ok(())
        }
        PluginCommands::Edit { plugin, editor } => {
            let path = registry.plugin_path_from_name_or_path(plugin)?;
            let editor_cmd = editor
                .clone()
                .or_else(|| std::env::var("EDITOR").ok())
                .unwrap_or_else(|| "vi".to_string());
            let status = std::process::Command::new(&editor_cmd)
                .arg(&path)
                .status()
                .with_context(|| format!("Failed to launch editor {}", editor_cmd))?;
            if !status.success() {
                anyhow::bail!("Editor exited with status {status}");
            }
            Ok(())
        }
        PluginCommands::GenerateExamples => {
            println!("Plugin examples are documented in README.");
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
    use super::{
        abbreviate_source_path, add_position_is_append, apply_global_file_add_context,
        build_add_text, choose_from_menu, completed_matches_date, completed_matches_pattern,
        editor_program_and_extra_args, effective_discovery_depth, find_actions,
        find_actions_with_options, format_saved_search, implicit_next_args,
        is_cancel_selection, is_taskpaper_search_filter, list_saved_searches,
        load_next_todo_files, load_update_todo_files, match_todo_path, next_actions,
        omnifocus_auxiliary_tags_suffix, open_command_for_target, parse_multi_action_edit_output,
        parse_multi_selection, parse_one_based_selection, parse_priority_value, parse_tag_datetime,
        parse_tag_input, parse_todo_specs, prompt_profile_path, read_scan_registry,
        restore_target_from_backup_path, run_archive, run_edit, run_move, run_plugin, run_prompt,
        run_saved, run_scan, run_tag, run_undo, run_update, save_next_search, saved_search_path,
        saved_search_slug, shell_quote_token, strip_trailing_note, update_seed_matches,
        write_scan_registry,
    };
    use crate::cli::{
        AddArgs, ArchiveArgs, Cli, Commands, CompletedArgs, EditArgs, FindArgs, MoveArgs, NextArgs,
        PluginCommands, PromptArgs, PromptCommands, SavedCommands, ScanArgs, TagArgs, UndoArgs,
        UpdateArgs,
    };
    use crate::io::xdg::TEST_ENV_MUTEX;
    use crate::models::action::Action;
    use crate::models::todo::TodoFile;
    use crate::output::duration::action_elapsed_seconds;
    use crate::parser::item_path::resolve_item_path;
    use clap::Parser;
    use std::collections::{HashMap, HashSet};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn base_next_args() -> NextArgs {
        NextArgs {
            filter: None,
            first_available: false,
            file: None,
            all: false,
            hidden: false,
            depth: None,
            in_todo: Vec::new(),
            done: false,
            tag: None,
            project: None,
            tagged: Vec::new(),
            priority: Vec::new(),
            search: Vec::new(),
            regex: false,
            exact: false,
            search_notes: true,
            no_search_notes: false,
            notes: false,
            no_notes: false,
            no_file: false,
            nest: false,
            omnifocus: false,
            plugin: None,
            input: None,
            output: None,
            divider: None,
            times: false,
            human: false,
            only_timed: false,
            json_times: false,
            only_times: false,
            save: None,
        }
    }

    fn base_find_args(query: &str) -> FindArgs {
        FindArgs {
            query: query.to_string(),
            regex: false,
            exact: false,
            depth: None,
            in_todo: Vec::new(),
            search_notes: true,
            no_search_notes: false,
            or_mode: false,
            project: None,
            tagged: Vec::new(),
            done: false,
            invert: false,
            save: None,
            notes: false,
            no_notes: false,
            nest: false,
            no_file: false,
            omnifocus: false,
            times: false,
            human: false,
            only_timed: false,
            json_times: false,
            only_times: false,
            plugin: None,
            input: None,
            output: None,
            divider: None,
        }
    }

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

    fn write_named_fixture_taskpaper(name: &str, content: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("valid monotonic time")
            .as_nanos();
        let seq = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        path.push(format!(
            "na_rust_update_named_{}_{}_{}_{}.taskpaper",
            name,
            std::process::id(),
            ts,
            seq
        ));
        fs::write(&path, content).expect("fixture should write");
        path
    }

    #[test]
    fn parse_multi_action_edit_output_preserves_blocks() {
        let raw = r#"# Instructions:
# ------ /tmp/a.taskpaper:1
One @na
note line

# ------ /tmp/a.taskpaper:3
Two

"#;
        let m = parse_multi_action_edit_output(raw).expect("parse");
        assert_eq!(m.len(), 2);
        let one = m
            .get(&("/tmp/a.taskpaper".to_string(), 1))
            .expect("first action");
        assert_eq!(one.0, "One @na");
        assert_eq!(one.1, vec!["note line".to_string()]);
        assert!(m.contains_key(&("/tmp/a.taskpaper".to_string(), 3)));
        assert_eq!(
            m.get(&("/tmp/a.taskpaper".to_string(), 3)).unwrap().0,
            "Two"
        );
    }

    #[test]
    fn editor_program_inserts_wait_flags_like_ruby() {
        let (p, a) = editor_program_and_extra_args("vim");
        assert_eq!(p, "vim");
        assert_eq!(a, vec!["-f"]);
        let (p2, a2) = editor_program_and_extra_args("vim -f");
        assert_eq!(p2, "vim");
        assert_eq!(a2, vec!["-f"]);
    }

    #[test]
    fn update_seed_matches_accepts_path_colon_line_query() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Ship feature @na
Inbox:
- Other
"#,
        );
        let todo = TodoFile::load(&path).expect("load");
        let ship = todo
            .actions()
            .into_iter()
            .find(|a| a.text.contains("Ship"))
            .expect("ship action");
        let args = base_update_args();
        let stem = path.file_name().expect("stem").to_str().expect("utf8");
        let q = format!("{}:{}", stem, ship.line_index + 1);
        let found = update_seed_matches(&[todo.clone()], &q, &args).expect("matches");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line_index, ship.line_index);

        let q_abs = format!("{}:{}", path.to_string_lossy(), ship.line_index + 1);
        let found_abs = update_seed_matches(&[todo], &q_abs, &args).expect("abs");
        assert_eq!(found_abs.len(), 1);

        let bogus_line = format!("{stem}:9999");
        assert!(
            update_seed_matches(&[TodoFile::load(&path).expect("re")], &bogus_line, &args)
                .expect("no line")
                .is_empty()
        );
    }

    #[test]
    fn update_path_colon_syntax_without_matching_file_skips_plain_substring_search() {
        let path = write_fixture_taskpaper("- Only mention x vaguely\n");
        let todo = TodoFile::load(&path).expect("load");
        let args = base_update_args();
        // Valid path:LINE shape; path `x` matches no todo file -> empty (no substring-match fallback).
        let hits = update_seed_matches(&[todo], "x:1", &args).expect("hits");
        assert!(hits.is_empty());
    }

    #[test]
    fn update_seed_path_colon_parses_trailing_line_so_colons_in_directories_work() {
        let dir = std::env::temp_dir().join(format!(
            "na_rust_pathcolon_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos(),
        ));
        let inner = dir.join("proj:colon");
        fs::create_dir_all(&inner).expect("mkdir");
        let tp = inner.join("list.taskpaper");
        fs::write(
            &tp,
            r#"Area:
  - Tagged @na
"#,
        )
        .expect("write todo");
        let todo = TodoFile::load(&tp).expect("load");
        let line_1based = todo
            .actions()
            .iter()
            .find(|a| a.text.contains("Tagged"))
            .expect("row")
            .line_index
            + 1;
        let full = tp.to_string_lossy().replace('\\', "/");
        let args = base_update_args();
        let hits = update_seed_matches(&[todo.clone()], &format!("{full}:{line_1based}"), &args)
            .expect("full path");
        assert_eq!(hits.len(), 1, "{full}:{line_1based}");
        let hits_short = update_seed_matches(
            &[todo],
            &format!(
                "{}:{line_1based}",
                tp.file_name().expect("stem").to_str().expect("utf8"),
            ),
            &args,
        )
        .expect("basename");
        assert_eq!(hits_short.len(), 1);
    }

    #[test]
    fn omnifocus_auxiliary_tags_suffix_like_ruby_output_children() {
        let a = Action {
            text: "Buy milk @na @priority(5) @due(today) @context(home-office)".into(),
            line_index: 1,
            project: None,
            project_chain: vec![],
            notes: vec![],
            tags: vec![
                "@na".into(),
                "@priority".into(),
                "@due".into(),
                "@context".into(),
            ],
            tag_values: HashMap::from([
                ("priority".into(), "5".into()),
                ("due".into(), "today".into()),
                ("context".into(), "home-office".into()),
            ]),
            done: false,
            due: None,
            source_file: "t.taskpaper".into(),
        };
        assert_eq!(
            omnifocus_auxiliary_tags_suffix(&a),
            " @tags(na,priority-5,context-home-office)"
        );

        let due_only = Action {
            text: "x @due(t)".into(),
            line_index: 0,
            project: None,
            project_chain: vec![],
            notes: vec![],
            tags: vec!["@due".into()],
            tag_values: HashMap::from([("due".into(), "t".into())]),
            done: false,
            due: None,
            source_file: "t.taskpaper".into(),
        };
        assert_eq!(omnifocus_auxiliary_tags_suffix(&due_only), "");
    }

    #[test]
    fn next_default_requires_na() {
        let path = write_fixture_taskpaper(
            r#"Inbox:
- Keep me @na
- Skip me
Archive:
- Archived @na
"#,
        );
        let todo = TodoFile::load(&path).expect("fixture should load");
        let args = base_next_args();
        let out = next_actions(&Cli::default(), &[todo], &args).expect("next should evaluate");
        fs::remove_file(path).ok();

        let texts = out.into_iter().map(|a| a.text).collect::<Vec<_>>();
        assert_eq!(texts.len(), 2);
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
        let mut args = base_next_args();
        args.filter = Some(r#"@search(@priority > 3 and @context contains "home")"#.to_string());
        let out = next_actions(&Cli::default(), &[todo], &args).expect("next should evaluate");
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
        let mut args = base_next_args();
        args.tagged = vec!["context".to_string()];
        args.first_available = true;
        let out = next_actions(&Cli::default(), &[todo], &args).expect("next should evaluate");
        fs::remove_file(path).ok();

        assert_eq!(out.len(), 2);
        assert!(out[0].text.contains("First A no tag"));
        assert!(out[1].text.contains("First B no tag"));
    }

    #[test]
    fn taskpaper_search_filter_skips_first_available_even_with_flag() {
        let path = write_fixture_taskpaper(
            r#"Work:
- First @context(office) @na
- Second @context(office) @na
"#,
        );
        let todo = TodoFile::load(&path).expect("fixture should load");
        let mut args = base_next_args();
        args.filter = Some(r#"@search(@context contains "office")"#.to_string());
        args.first_available = true;
        let out = next_actions(&Cli::default(), &[todo], &args).expect("next should evaluate");
        fs::remove_file(path).ok();
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn global_depth_applies_when_command_depth_unset() {
        let cli = Cli {
            depth: Some(2),
            ..Default::default()
        };
        assert_eq!(effective_discovery_depth(&cli, None, 5), 2);
        assert_eq!(effective_discovery_depth(&cli, Some(4), 5), 4);
    }

    #[test]
    fn add_with_global_file_uses_cwd_as_project() {
        let sandbox = std::env::temp_dir().join(format!(
            "na-add-cwd-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&sandbox).expect("sandbox");
        let cwd = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&sandbox).expect("chdir");
        let path = sandbox.join("todo.taskpaper");
        fs::write(&path, "Inbox:\n").expect("write");
        let cli = Cli {
            global_file: Some(path.clone()),
            cwd_as: "project".to_string(),
            ..Default::default()
        };
        let (project, text) = apply_global_file_add_context(&cli, "Inbox", "Ship feature @na");
        std::env::set_current_dir(&cwd).expect("restore cwd");
        fs::remove_dir_all(&sandbox).ok();
        assert_eq!(project, sandbox.file_name().unwrap().to_string_lossy());
        assert!(text.contains("@na"));
    }

    #[test]
    fn cli_parsed_search_with_available_does_not_dedupe_per_project() {
        let path = std::env::temp_dir().join(format!(
            "na-search-avail-{}",
            FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(
            &path,
            r#"Work:
- Prepare slides @na @priority(5) @context(home-office)
- Review inbox @context(office) @testupdate @na
"#,
        )
        .expect("write fixture");
        let cli = Cli::parse_from([
            "na",
            "next",
            r#"@search(@context contains "office")"#,
            "--file",
            path.to_str().expect("utf8 path"),
            "--available",
        ]);
        let args = match &cli.command {
            Some(Commands::Next(args)) => args,
            _ => panic!("expected next command"),
        };
        assert!(args.first_available);
        assert!(is_taskpaper_search_filter(args));
        let todo = TodoFile::load(&path).expect("load");
        let out = next_actions(&cli, &[todo], args).expect("evaluate");
        fs::remove_file(path).ok();
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn next_first_available_with_tagged_filter_keeps_one_per_project() {
        let path = write_fixture_taskpaper(
            r#"Dev:
- First task @context(home)
- Second task @context(home)
- Third @na @context(home)
"#,
        );
        let todo = TodoFile::load(&path).expect("fixture should load");
        let mut args = base_next_args();
        args.tagged = vec!["context".to_string()];
        args.first_available = true;
        let out = next_actions(&Cli::default(), &[todo], &args).expect("next should evaluate");
        fs::remove_file(path).ok();

        assert_eq!(out.len(), 1);
        assert!(out[0].text.contains("First task"));
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
        let out = find_actions(&[todo], r#"@search(@priority > 3)"#).expect("find should evaluate");
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
    fn find_with_regex_matches_action_text() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Build API v2
- Write docs
"#,
        );
        let todo = TodoFile::load(&path).expect("fixture should load");
        let mut args = base_find_args(r"API v\d");
        args.regex = true;
        let out = find_actions_with_options(&[todo], &args).expect("find should evaluate");
        fs::remove_file(path).ok();
        assert_eq!(out.len(), 1);
        assert!(out[0].text.contains("Build API"));
    }

    #[test]
    fn find_with_or_mode_matches_any_term() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Build backend service
- Plan roadmap
"#,
        );
        let todo = TodoFile::load(&path).expect("fixture should load");
        let mut args = base_find_args("backend docs");
        args.or_mode = true;
        let out = find_actions_with_options(&[todo], &args).expect("find should evaluate");
        fs::remove_file(path).ok();
        assert_eq!(out.len(), 1);
        assert!(out[0].text.contains("Build backend"));
    }

    #[test]
    fn find_search_notes_can_be_disabled() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Visible task
    hidden needle in note
"#,
        );
        let todo = TodoFile::load(&path).expect("fixture should load");
        let mut args = base_find_args("needle");
        args.no_search_notes = true;
        let out = find_actions_with_options(&[todo], &args).expect("find should evaluate");
        fs::remove_file(path).ok();
        assert!(out.is_empty());
    }

    #[test]
    fn undo_restores_latest_backup_for_global_file() {
        let _env_guard = TEST_ENV_MUTEX.lock().expect("env mutex");
        let path = write_named_fixture_taskpaper(
            "undo_restore",
            r#"Inbox:
- Current content
"#,
        );
        let backup = crate::io::fs::backup_path(&path);
        if let Some(parent) = backup.parent() {
            fs::create_dir_all(parent).expect("backup parent");
        }
        fs::write(
            &backup,
            r#"Inbox:
- Restored content
"#,
        )
        .expect("backup write");
        let cli = Cli {
            global_file: Some(path.clone()),
            no_color: true,
            ..Default::default()
        };
        let args = UndoArgs { select: false };
        run_undo(&cli, &args).expect("undo should restore backup");
        let restored = fs::read_to_string(&path).expect("read restored file");
        fs::remove_file(path).ok();
        fs::remove_file(backup).ok();
        assert!(restored.contains("Restored content"));
    }

    #[test]
    fn undo_select_non_interactive_prefers_latest_backup() {
        let _env_guard = TEST_ENV_MUTEX.lock().expect("env mutex");
        let mut data_home = std::env::temp_dir();
        data_home.push(format!(
            "na_rust_undo_select_{}_{}",
            std::process::id(),
            FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let backup_root = data_home.join("na").join("backup").join("tmp");
        fs::create_dir_all(&backup_root).expect("backup root");
        let older = backup_root.join("older.taskpaper.bak");
        let newer = backup_root.join("newer.taskpaper.bak");
        fs::write(&older, "Inbox:\n- older\n").expect("older write");
        std::thread::sleep(std::time::Duration::from_millis(2));
        fs::write(&newer, "Inbox:\n- newer\n").expect("newer write");
        std::env::set_var("XDG_DATA_HOME", &data_home);
        let cli = Cli {
            global_file: None,
            no_color: true,
            ..Default::default()
        };
        run_undo(&cli, &UndoArgs { select: true }).expect("undo select should succeed");
        let restored = fs::read_to_string("/tmp/newer.taskpaper").expect("restored newer");
        std::env::remove_var("XDG_DATA_HOME");
        fs::remove_file("/tmp/newer.taskpaper").ok();
        fs::remove_dir_all(data_home).ok();
        assert!(restored.contains("newer"), "{restored}");
    }

    #[test]
    fn restore_target_from_backup_path_strips_backup_suffix() {
        let backup = crate::io::xdg::na_backup_dir()
            .join("tmp")
            .join("demo.taskpaper.bak");
        let target = restore_target_from_backup_path(&backup);
        assert!(target.to_string_lossy().ends_with("/tmp/demo.taskpaper"));
    }

    #[test]
    fn next_file_option_loads_only_target_file() {
        let path = write_fixture_taskpaper(
            r#"Inbox:
- Keep me @na
"#,
        );
        let cli = Cli::parse_from(["na", "next"]);
        let mut args = base_next_args();
        args.file = Some(path.clone());

        let files = load_next_todo_files(&cli, &args).expect("next file loading should work");
        fs::remove_file(path).ok();

        assert_eq!(files.len(), 1);
    }

    #[test]
    fn next_available_alias_invokes_next_command() {
        let cli = Cli::parse_from(["na", "next", "--available"]);
        match cli.command {
            Some(Commands::Next(args)) => assert!(args.first_available),
            _ => panic!("expected next command"),
        }
    }

    #[test]
    fn next_done_flag_includes_done_items() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Done thing @na @done
"#,
        );
        let todo = TodoFile::load(&path).expect("fixture should load");
        let mut args = base_next_args();
        args.done = true;
        let out = next_actions(&Cli::default(), &[todo], &args).expect("next should evaluate");
        fs::remove_file(path).ok();
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn next_search_can_match_notes() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Action @na
    hidden needle
"#,
        );
        let todo = TodoFile::load(&path).expect("fixture should load");
        let mut args = base_next_args();
        args.search = vec!["needle".to_string()];
        let out = next_actions(&Cli::default(), &[todo], &args).expect("next should evaluate");
        fs::remove_file(path).ok();
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn next_search_respects_no_search_notes() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Visible @na
    needle in note
"#,
        );
        let todo = TodoFile::load(&path).expect("fixture should load");
        let mut args = base_next_args();
        args.search = vec!["needle".to_string()];
        args.no_search_notes = true;
        let out = next_actions(&Cli::default(), &[todo], &args).expect("next should evaluate");
        fs::remove_file(path).ok();
        assert!(out.is_empty());
    }

    #[test]
    fn saved_search_slug_normalizes_and_collapses_separators() {
        assert_eq!(
            saved_search_slug("  Weekly Focus / Work  "),
            "weekly_focus_work"
        );
        assert_eq!(saved_search_slug("___"), "");
        assert_eq!(saved_search_slug("Roadmap v2"), "roadmap_v2");
    }

    #[test]
    fn format_saved_search_quotes_values_and_omits_save_flag() {
        let mut args = base_next_args();
        args.first_available = true;
        args.project = Some("Client Alpha".to_string());
        args.search = vec!["one two".to_string()];
        args.file = Some(PathBuf::from("/tmp/with space.taskpaper"));
        args.save = Some("My Saved Search".to_string());
        let formatted = format_saved_search(&args);
        assert!(
            formatted.starts_with("na next --first-available"),
            "{formatted}"
        );
        assert!(
            formatted.contains("--project 'Client Alpha'"),
            "{formatted}"
        );
        assert!(formatted.contains("--search 'one two'"), "{formatted}");
        assert!(
            formatted.contains("--file '/tmp/with space.taskpaper'"),
            "{formatted}"
        );
        assert!(!formatted.contains("--save"), "{formatted}");
    }

    #[test]
    fn shell_quote_token_escapes_single_quotes() {
        assert_eq!(shell_quote_token("plain-token"), "plain-token");
        assert_eq!(shell_quote_token("needs quote"), "'needs quote'");
        assert_eq!(shell_quote_token("can't"), "'can'\\''t'");
    }

    #[test]
    fn save_next_search_writes_under_data_dir_searches() {
        let _env_guard = TEST_ENV_MUTEX.lock().expect("env mutex");
        let xdg = std::env::temp_dir().join(format!(
            "na_rust_saved_search_fixture_{}_{}",
            std::process::id(),
            FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&xdg).expect("xdg fixture should create");
        std::env::set_var("XDG_DATA_HOME", &xdg);

        let mut args = base_next_args();
        args.project = Some("Client Alpha".to_string());
        args.search = vec!["deep work".to_string()];
        save_next_search(&args, "Weekly Focus").expect("save should succeed");

        let saved_path = xdg.join("na").join("searches").join("weekly_focus.txt");
        let saved = fs::read_to_string(&saved_path).expect("saved command should exist");
        std::env::remove_var("XDG_DATA_HOME");
        fs::remove_dir_all(&xdg).ok();

        assert!(saved.starts_with("na next"), "{saved}");
        assert!(saved.contains("--project 'Client Alpha'"), "{saved}");
        assert!(saved.contains("--search 'deep work'"), "{saved}");
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

    #[test]
    fn elapsed_seconds_parses_started_and_done_tags() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Timed @started(2026-04-01 10:00) @done(2026-04-01 11:30)
"#,
        );
        let todo = TodoFile::load(&path).expect("fixture should load");
        fs::remove_file(path).ok();
        let elapsed = action_elapsed_seconds(&todo.actions()[0]).expect("duration should parse");
        assert_eq!(elapsed, 5400);
    }

    #[test]
    fn only_timed_filters_out_missing_time_tags() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Timed @na @started(2026-04-01 10:00) @done(2026-04-01 11:00)
- Untimed @na
"#,
        );
        let todo = TodoFile::load(&path).expect("fixture should load");
        let mut args = base_next_args();
        args.only_timed = true;
        let out = next_actions(&Cli::default(), &[todo], &args).expect("next should evaluate");
        fs::remove_file(path).ok();
        assert_eq!(out.len(), 1);
        assert!(out[0].text.contains("Timed"));
    }

    fn base_add_args(text: &str) -> AddArgs {
        AddArgs {
            text: text.to_string(),
            started: None,
            end: None,
            duration: None,
            project: "Inbox".to_string(),
            at: None,
            in_todo: Vec::new(),
            priority: None,
            tag: None,
            no_next_tag: false,
            file: None,
            finish: false,
            depth: None,
            note: false,
        }
    }

    fn base_update_args() -> UpdateArgs {
        UpdateArgs {
            query: None,
            tag: Vec::new(),
            untag: Vec::new(),
            done: false,
            file: None,
            depth: 1,
            in_todo: Vec::new(),
            search: Vec::new(),
            all: false,
            replace: None,
            to: None,
            project: None,
            tagged: Vec::new(),
            regex: false,
            exact: false,
            search_notes: true,
            no_search_notes: false,
            priority: None,
            at: None,
            archive: false,
            edit: false,
            editor: None,
            delete: false,
            restore: false,
            note: Vec::new(),
            overwrite_notes: false,
            started: None,
            end: None,
            duration: None,
            plugin: None,
            input: None,
            output: None,
            divider: None,
        }
    }

    fn base_completed_args() -> CompletedArgs {
        CompletedArgs {
            pattern: Vec::new(),
            before: None,
            on: None,
            after: None,
            or_mode: false,
            depth: None,
            in_todo: Vec::new(),
            notes: false,
            no_notes: false,
            search_notes: true,
            no_search_notes: false,
            project: None,
            tagged: Vec::new(),
            nest: false,
            omnifocus: false,
            no_file: false,
            save: None,
        }
    }

    fn base_archive_args() -> ArchiveArgs {
        ArchiveArgs {
            query: None,
            done: false,
            file: None,
            depth: 1,
            tagged: Vec::new(),
            project: None,
            in_todo: Vec::new(),
            search: Vec::new(),
            regex: false,
            exact: false,
            all: false,
            note: Vec::new(),
            overwrite_notes: false,
        }
    }

    #[test]
    fn add_helpers_parse_priority_and_position() {
        assert_eq!(parse_priority_value(Some("h")), Some(5));
        assert_eq!(parse_priority_value(Some("m")), Some(3));
        assert_eq!(parse_priority_value(Some("l")), Some(1));
        assert_eq!(parse_priority_value(Some("4")), Some(4));
        assert_eq!(parse_priority_value(Some("9")), None);
        assert!(!add_position_is_append(Some("start")));
        assert!(add_position_is_append(Some("end")));
    }

    #[test]
    fn add_build_text_applies_tags_and_timing_flags() {
        let mut args = base_add_args("Ship feature @na (capture note)");
        args.priority = Some("h".to_string());
        args.started = Some("2026-04-20 09:00".to_string());
        args.end = Some("2026-04-20 10:00".to_string());
        args.duration = Some("1h".to_string());
        let text = build_add_text(&Cli::default(), &args);
        assert!(text.contains("@priority(5)"), "{text}");
        assert!(text.contains("@na"), "{text}");
        assert!(text.contains("@started(2026-04-20 09:00)"), "{text}");
        assert!(text.contains("@done(2026-04-20 10:00)"), "{text}");
        assert!(text.contains("@duration(1h)"), "{text}");
        assert!(!text.contains("(capture note)"), "{text}");
    }

    #[test]
    fn add_strip_trailing_note_extracts_parenthetical_suffix() {
        let (text, note) = strip_trailing_note("Action body (quick note)");
        assert_eq!(text, "Action body");
        assert_eq!(note.as_deref(), Some("quick note"));
    }

    #[test]
    fn add_in_todo_token_matching_supports_required_and_negated_tokens() {
        let specs = parse_todo_specs(&["work,+client,-archive".to_string()]);
        assert!(match_todo_path("/tmp/work-client.taskpaper", &specs));
        assert!(!match_todo_path("/tmp/work.taskpaper", &specs));
        assert!(!match_todo_path(
            "/tmp/work-client-archive.taskpaper",
            &specs
        ));
    }

    #[test]
    fn add_item_path_resolution_supports_child_descendant_and_wildcard() {
        let projects = vec![
            "Inbox:Errands".to_string(),
            "Work:ClientA:Ops".to_string(),
            "Work:ClientB:Bugs".to_string(),
        ];
        let child = resolve_item_path("/Work/ClientA", &projects);
        assert_eq!(child, vec!["Work:ClientA".to_string()]);
        let desc = resolve_item_path("//Bugs", &projects);
        assert_eq!(desc, vec!["Work:ClientB:Bugs".to_string()]);
        let wildcard = resolve_item_path("/Work/*/Ops", &projects);
        assert_eq!(wildcard, vec!["Work:ClientA:Ops".to_string()]);
    }

    #[test]
    fn parse_one_based_selection_accepts_valid_menu_input() {
        assert_eq!(parse_one_based_selection("1", 3), Some(0));
        assert_eq!(parse_one_based_selection(" 3 ", 3), Some(2));
        assert_eq!(parse_one_based_selection("0", 3), None);
        assert_eq!(parse_one_based_selection("4", 3), None);
        assert_eq!(parse_one_based_selection("abc", 3), None);
    }

    #[test]
    fn choose_from_menu_non_interactive_defaults_to_first() {
        let idx = choose_from_menu(
            "Pick one",
            &["alpha".to_string(), "beta".to_string()],
            false,
        )
        .expect("non-interactive selection should work");
        assert_eq!(idx, 0);
    }

    #[test]
    fn menu_cancel_selection_supports_blank_and_quit_tokens() {
        assert!(is_cancel_selection(""));
        assert!(is_cancel_selection("   "));
        assert!(is_cancel_selection("q"));
        assert!(is_cancel_selection("Q"));
        assert!(is_cancel_selection("quit"));
        assert!(!is_cancel_selection("1"));
        assert!(!is_cancel_selection("nope"));
    }

    #[test]
    fn parse_multi_selection_supports_single_range_and_all() {
        assert_eq!(parse_multi_selection("1,3-4", 5), Some(vec![0, 2, 3]));
        assert_eq!(parse_multi_selection("all", 3), Some(vec![0, 1, 2]));
        assert_eq!(parse_multi_selection("0", 3), None);
        assert_eq!(parse_multi_selection("3-1", 3), None);
    }

    #[test]
    fn parse_tag_input_normalizes_prefix_and_splits_commas() {
        assert_eq!(
            parse_tag_input("home,@today , priority(3)"),
            vec![
                "@home".to_string(),
                "@today".to_string(),
                "@priority(3)".to_string()
            ]
        );
    }

    #[test]
    fn implicit_no_command_next_uses_depth_three() {
        let args = implicit_next_args();
        assert_eq!(args.depth, Some(3));
    }

    #[test]
    fn run_update_path_colon_line_adds_tag_without_reordering() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Ship feature @na
- Triage bug
    has note
Inbox:
- Capture idea
"#,
        );
        let cli = Cli {
            global_file: Some(path.clone()),
            no_color: true,
            ..Default::default()
        };
        let mut args = base_update_args();
        args.query = Some(format!(
            "{}:2",
            path.file_name().expect("stem").to_string_lossy()
        ));
        args.tag = vec!["patched".to_string()];
        args.all = true;

        run_update(&cli, &args).expect("run_update should succeed");
        let updated = fs::read_to_string(&path).expect("fixture should read");
        fs::remove_file(path).ok();

        assert!(
            updated.contains("\t- Ship feature @na @patched\n- Triage bug"),
            "{updated}"
        );
        assert!(!updated.contains("Triage bug @patched"), "{updated}");
    }

    #[test]
    fn run_update_non_interactive_query_branch_updates_matching_actions() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Match me
- Skip me
"#,
        );
        let cli = Cli {
            global_file: Some(path.clone()),
            no_color: true,
            ..Default::default()
        };
        let mut args = base_update_args();
        args.query = Some("Match".to_string());
        args.tag = vec!["@today".to_string()];

        run_update(&cli, &args).expect("run_update should succeed");
        let updated = fs::read_to_string(&path).expect("fixture should read");
        fs::remove_file(path).ok();

        assert!(updated.contains("- Match me @today"), "{updated}");
        assert!(updated.contains("- Skip me\n"), "{updated}");
    }

    #[test]
    fn run_update_non_interactive_search_fallback_branch_updates_matches() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Needs search token
- Unrelated action
"#,
        );
        let cli = Cli {
            global_file: Some(path.clone()),
            no_color: true,
            ..Default::default()
        };
        let mut args = base_update_args();
        args.search = vec!["search token".to_string()];
        args.done = true;

        run_update(&cli, &args).expect("run_update should succeed");
        let updated = fs::read_to_string(&path).expect("fixture should read");
        fs::remove_file(path).ok();

        assert!(updated.contains("- Needs search token @done("), "{updated}");
        assert!(updated.contains("- Unrelated action\n"), "{updated}");
    }

    #[test]
    fn run_update_non_interactive_no_query_with_direct_flag_updates_all_candidates() {
        let path = write_fixture_taskpaper(
            r#"Work:
- First
- Second
"#,
        );
        let cli = Cli {
            global_file: Some(path.clone()),
            no_color: true,
            ..Default::default()
        };
        let mut args = base_update_args();
        args.tag = vec!["@bulk".to_string()];

        run_update(&cli, &args).expect("run_update should succeed");
        let updated = fs::read_to_string(&path).expect("fixture should read");
        fs::remove_file(path).ok();

        assert!(updated.contains("\t- First @bulk"), "{updated}");
        assert!(updated.contains("\t- Second @bulk"), "{updated}");
    }

    #[test]
    fn run_update_replace_and_note_overwrite_applies_to_matches() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Old title
    old note
"#,
        );
        let cli = Cli {
            global_file: Some(path.clone()),
            no_color: true,
            ..Default::default()
        };
        let mut args = base_update_args();
        args.query = Some("Old title".to_string());
        args.replace = Some("New title @na".to_string());
        args.note = vec!["fresh note".to_string()];
        args.overwrite_notes = true;

        run_update(&cli, &args).expect("run_update should succeed");
        let updated = fs::read_to_string(&path).expect("fixture should read");
        fs::remove_file(path).ok();
        assert!(updated.contains("- New title @na"), "{updated}");
        assert!(updated.contains("\tfresh note"), "{updated}");
        assert!(!updated.contains("old note"), "{updated}");
    }

    #[test]
    fn run_edit_non_interactive_replaces_action_text() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Before edit @na
"#,
        );
        let cli = Cli {
            global_file: Some(path.clone()),
            no_color: true,
            ..Default::default()
        };
        let args = EditArgs {
            query: Some("Before edit".to_string()),
            text: "After edit @na".to_string(),
            file: None,
            depth: 1,
            in_todo: Vec::new(),
            search: Vec::new(),
            all: true,
        };
        run_edit(&cli, &args).expect("run_edit should succeed");
        let updated = fs::read_to_string(&path).expect("fixture should read");
        fs::remove_file(path).ok();
        assert!(updated.contains("- After edit @na"), "{updated}");
        assert!(!updated.contains("Before edit"), "{updated}");
    }

    #[test]
    fn run_update_delete_removes_matching_action() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Keep me
- Remove me
"#,
        );
        let cli = Cli {
            global_file: Some(path.clone()),
            no_color: true,
            ..Default::default()
        };
        let mut args = base_update_args();
        args.query = Some("Remove me".to_string());
        args.delete = true;
        run_update(&cli, &args).expect("run_update should succeed");
        let updated = fs::read_to_string(&path).expect("fixture should read");
        fs::remove_file(path).ok();
        assert!(updated.contains("- Keep me"), "{updated}");
        assert!(!updated.contains("Remove me"), "{updated}");
    }

    #[test]
    fn run_update_move_to_project_rehomes_action() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Move me @na
Inbox:
- Keep here
"#,
        );
        let cli = Cli {
            global_file: Some(path.clone()),
            no_color: true,
            ..Default::default()
        };
        let mut args = base_update_args();
        args.query = Some("Move me".to_string());
        args.to = Some("Inbox".to_string());
        run_update(&cli, &args).expect("run_update should succeed");
        let updated = fs::read_to_string(&path).expect("fixture should read");
        fs::remove_file(path).ok();
        assert!(
            updated.contains("Inbox:\n\t- Move me @na\n- Keep here"),
            "{updated}"
        );
    }

    #[test]
    fn run_move_at_start_places_action_first_in_target_project() {
        let path = write_fixture_taskpaper(
            r#"Source:
- Move me @na
Inbox:
- Existing
"#,
        );
        let cli = Cli {
            global_file: Some(path.clone()),
            no_color: true,
            ..Default::default()
        };
        let args = MoveArgs {
            query: Some("Move me".to_string()),
            to: "Inbox".to_string(),
            at: Some("start".to_string()),
            from: None,
            in_todo: Vec::new(),
            file: None,
            depth: 1,
            search: Vec::new(),
            search_notes: true,
            tagged: Vec::new(),
            regex: false,
            exact: false,
            all: true,
        };
        run_move(&cli, &args).expect("move should succeed");
        let updated = fs::read_to_string(&path).expect("fixture should read");
        fs::remove_file(path).ok();
        assert!(
            updated.contains("Inbox:\n\t- Move me @na\n- Existing"),
            "{updated}"
        );
    }

    #[test]
    fn run_move_at_end_places_action_last_in_target_project() {
        let path = write_fixture_taskpaper(
            r#"Source:
- Move me @na
Inbox:
- Existing
"#,
        );
        let cli = Cli {
            global_file: Some(path.clone()),
            no_color: true,
            ..Default::default()
        };
        let args = MoveArgs {
            query: Some("Move me".to_string()),
            to: "Inbox".to_string(),
            at: Some("end".to_string()),
            from: None,
            in_todo: Vec::new(),
            file: None,
            depth: 1,
            search: Vec::new(),
            search_notes: true,
            tagged: Vec::new(),
            regex: false,
            exact: false,
            all: true,
        };
        run_move(&cli, &args).expect("move should succeed");
        let updated = fs::read_to_string(&path).expect("fixture should read");
        fs::remove_file(path).ok();
        assert!(
            updated.contains("Inbox:\n- Existing\n\t- Move me @na"),
            "{updated}"
        );
    }

    #[test]
    fn run_update_regex_query_matches_and_updates() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Ship API v2
- Write docs
"#,
        );
        let cli = Cli {
            global_file: Some(path.clone()),
            no_color: true,
            ..Default::default()
        };
        let mut args = base_update_args();
        args.query = Some("API v\\d".to_string());
        args.regex = true;
        args.tag = vec!["@today".to_string()];
        run_update(&cli, &args).expect("run_update should succeed");
        let updated = fs::read_to_string(&path).expect("fixture should read");
        fs::remove_file(path).ok();
        assert!(updated.contains("- Ship API v2 @today"), "{updated}");
        assert!(updated.contains("- Write docs\n"), "{updated}");
    }

    #[test]
    fn run_update_applies_timing_and_priority_tags() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Tune query @na
"#,
        );
        let cli = Cli {
            global_file: Some(path.clone()),
            no_color: true,
            ..Default::default()
        };
        let mut args = base_update_args();
        args.query = Some("Tune query".to_string());
        args.priority = Some("h".to_string());
        args.started = Some("2026-04-20 09:00".to_string());
        args.end = Some("2026-04-20 10:30".to_string());
        args.duration = Some("90m".to_string());
        run_update(&cli, &args).expect("run_update should succeed");
        let updated = fs::read_to_string(&path).expect("fixture should read");
        fs::remove_file(path).ok();
        assert!(updated.contains("@priority(5)"), "{updated}");
        assert!(updated.contains("@started(2026-04-20 09:00)"), "{updated}");
        assert!(updated.contains("@done(2026-04-20 10:30)"), "{updated}");
        assert!(updated.contains("@duration(90m)"), "{updated}");
    }

    #[test]
    fn run_update_archive_flag_moves_selected_action_to_archive() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Archive me
- Keep me
"#,
        );
        let cli = Cli {
            global_file: Some(path.clone()),
            no_color: true,
            ..Default::default()
        };
        let mut args = base_update_args();
        args.query = Some("Archive me".to_string());
        args.archive = true;
        args.all = true;
        run_update(&cli, &args).expect("run_update archive should succeed");
        let updated = fs::read_to_string(&path).expect("fixture should read");
        fs::remove_file(path).ok();
        assert!(
            updated.contains("Archive:\n- Archive me @done("),
            "{updated}"
        );
        assert!(updated.contains("- Keep me"), "{updated}");
    }

    #[test]
    fn open_command_uses_app_override_when_present() {
        let path = PathBuf::from("/tmp/todo.taskpaper");
        let (program, args) = open_command_for_target(&path, Some("nano"), Some("TextEdit"));
        #[cfg(target_os = "macos")]
        {
            assert_eq!(program, "open");
            assert_eq!(args, vec!["-a", "TextEdit", "/tmp/todo.taskpaper"]);
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(program, "TextEdit");
            assert_eq!(args, vec!["/tmp/todo.taskpaper"]);
        }
    }

    #[test]
    fn scan_registry_round_trip_reads_written_paths() {
        let _env_guard = TEST_ENV_MUTEX.lock().expect("env mutex");
        let mut data_home = std::env::temp_dir();
        data_home.push(format!(
            "na_rust_scan_registry_{}_{}",
            std::process::id(),
            FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&data_home).expect("data home");
        std::env::set_var("XDG_DATA_HOME", &data_home);
        let mut expected = HashSet::new();
        expected.insert("/tmp/a.taskpaper".to_string());
        expected.insert("/tmp/b.taskpaper".to_string());
        write_scan_registry(&expected).expect("registry write");
        let read_back = read_scan_registry().expect("registry read");
        std::env::remove_var("XDG_DATA_HOME");
        fs::remove_dir_all(data_home).ok();
        assert_eq!(read_back, expected);
    }

    #[test]
    fn run_scan_dry_run_does_not_mutate_registry_file() {
        let _env_guard = TEST_ENV_MUTEX.lock().expect("env mutex");
        let cwd = std::env::current_dir().expect("cwd");
        let mut sandbox = std::env::temp_dir();
        sandbox.push(format!(
            "na_rust_scan_sandbox_{}_{}",
            std::process::id(),
            FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&sandbox).expect("sandbox");
        fs::write(sandbox.join("todo.taskpaper"), "Inbox:\n- One\n").expect("fixture");
        let mut data_home = std::env::temp_dir();
        data_home.push(format!(
            "na_rust_scan_data_{}_{}",
            std::process::id(),
            FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&data_home).expect("data home");
        std::env::set_var("XDG_DATA_HOME", &data_home);
        let mut seeded = HashSet::new();
        seeded.insert("/tmp/stale.taskpaper".to_string());
        write_scan_registry(&seeded).expect("seed registry");
        let cli = Cli {
            global_file: None,
            no_color: true,
            ..Default::default()
        };
        let args = ScanArgs {
            depth: 3,
            prune: true,
            hidden: false,
            dry_run: true,
        };
        std::env::set_current_dir(&sandbox).expect("enter sandbox");
        run_scan(&cli, &args).expect("scan dry-run");
        std::env::set_current_dir(cwd).expect("restore cwd");
        let after = read_scan_registry().expect("read after");
        std::env::remove_var("XDG_DATA_HOME");
        fs::remove_dir_all(sandbox).ok();
        fs::remove_dir_all(data_home).ok();
        assert_eq!(after, seeded);
    }

    #[test]
    fn prompt_install_is_idempotent_for_profile_file() {
        let _env_guard = TEST_ENV_MUTEX.lock().expect("env mutex");
        let mut path = std::env::temp_dir();
        path.push(format!(
            "na_rust_prompt_profile_{}_{}",
            std::process::id(),
            FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::env::set_var("NA_PROMPT_PROFILE", &path);
        let args = PromptArgs {
            command: Some(PromptCommands::Install),
        };
        let cli = Cli::default();
        run_prompt(&cli, &args).expect("first install");
        run_prompt(&cli, &args).expect("second install");
        let content = fs::read_to_string(&path).expect("profile read");
        std::env::remove_var("NA_PROMPT_PROFILE");
        fs::remove_file(path).ok();
        assert_eq!(
            content.matches("prompt hook for na").count()
                + content.matches("Prompt Command").count(),
            1
        );
    }

    #[test]
    fn prompt_profile_path_honors_override_env() {
        let _env_guard = TEST_ENV_MUTEX.lock().expect("env mutex");
        std::env::set_var("NA_PROMPT_PROFILE", "/tmp/na_prompt_profile_test");
        let path = prompt_profile_path().expect("profile path");
        std::env::remove_var("NA_PROMPT_PROFILE");
        assert_eq!(path, PathBuf::from("/tmp/na_prompt_profile_test"));
    }

    #[test]
    fn load_update_todo_files_prefers_explicit_file_over_global_or_discovery() {
        let explicit = write_named_fixture_taskpaper("explicit", "Inbox:\n- Explicit\n");
        let global = write_named_fixture_taskpaper("global", "Inbox:\n- Global\n");
        let cli = Cli {
            global_file: Some(global.clone()),
            no_color: true,
            ..Default::default()
        };
        let mut args = base_update_args();
        args.file = Some(explicit.clone());

        let files = load_update_todo_files(&cli, &args).expect("load should succeed");
        fs::remove_file(explicit.clone()).ok();
        fs::remove_file(global.clone()).ok();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, explicit);
    }

    #[test]
    fn load_update_todo_files_filters_by_in_todo_tokens() {
        let cwd = std::env::current_dir().expect("cwd should resolve");
        let mut sandbox = cwd.clone();
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("valid monotonic time")
            .as_nanos();
        let seq = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        sandbox.push(format!("na_rust_update_filter_fixture_{}_{}", ts, seq));
        fs::create_dir_all(&sandbox).expect("sandbox should create");
        let keep = sandbox.join("keep_work_client.taskpaper");
        let drop = sandbox.join("drop_home_archive.taskpaper");
        fs::write(&keep, "Inbox:\n- Keep\n").expect("keep fixture should write");
        fs::write(&drop, "Inbox:\n- Drop\n").expect("drop fixture should write");

        let cli = Cli {
            global_file: None,
            no_color: true,
            ..Default::default()
        };
        let mut args = base_update_args();
        // Require "work", optional "client", and exclude "archive".
        args.in_todo = vec!["work,+client,-archive".to_string()];
        args.depth = 3;

        std::env::set_current_dir(&sandbox).expect("should enter sandbox");
        let files = load_update_todo_files(&cli, &args).expect("load should succeed");
        std::env::set_current_dir(&cwd).expect("should restore cwd");
        fs::remove_dir_all(&sandbox).ok();

        let loaded_paths = files
            .iter()
            .map(|f| f.path.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert!(
            loaded_paths.iter().any(|p| p.contains("keep_work_client")),
            "{loaded_paths:?}"
        );
        assert!(
            !loaded_paths.iter().any(|p| p.contains("drop_home_archive")),
            "{loaded_paths:?}"
        );
    }

    #[test]
    fn completed_matches_done_and_pattern_and_date_filters() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Done match feature @done(2026-04-20 10:00)
- Done other @done(2026-04-18 09:00)
- Open feature
"#,
        );
        let todo = TodoFile::load(&path).expect("fixture should load");
        fs::remove_file(path).ok();
        let mut args = base_completed_args();
        args.pattern = vec!["feature".to_string()];
        args.after = Some("2026-04-19".to_string());
        let mut actions: Vec<Action> = vec![
            todo.actions()[0].clone(),
            todo.actions()[1].clone(),
            todo.actions()[2].clone(),
        ];
        actions.retain(|a| a.done);
        actions
            .retain(|a| completed_matches_pattern(a, &args.pattern, args.effective_search_notes()));
        let after = args.after.as_deref().and_then(parse_tag_datetime);
        actions.retain(|a| completed_matches_date(a, None, None, after, args.or_mode));
        assert_eq!(actions.len(), 1);
        assert!(actions[0].text.contains("Done match feature"));
    }

    #[test]
    fn completed_finished_alias_invokes_completed_command() {
        let cli = Cli::parse_from(["na", "finished", "--on", "2026-04-20", "feature"]);
        match cli.command {
            Some(Commands::Completed(args)) => {
                assert_eq!(args.on.as_deref(), Some("2026-04-20"));
                assert_eq!(args.pattern, vec!["feature".to_string()]);
            }
            _ => panic!("expected completed command"),
        }
    }

    #[test]
    fn list_saved_searches_returns_sorted_stems() {
        let xdg = std::env::temp_dir().join(format!(
            "na_rust_saved_list_fixture_{}_{}",
            std::process::id(),
            FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let searches = xdg.join("na").join("searches");
        fs::create_dir_all(&searches).expect("search dir should create");
        fs::write(searches.join("beta.txt"), "na next").expect("beta should write");
        fs::write(searches.join("alpha.txt"), "na next").expect("alpha should write");
        fs::write(searches.join("ignore.md"), "noop").expect("non-search should write");
        std::env::set_var("XDG_DATA_HOME", &xdg);

        let listed = list_saved_searches().expect("listing should work");
        std::env::remove_var("XDG_DATA_HOME");
        fs::remove_dir_all(&xdg).ok();

        assert_eq!(listed, vec!["alpha".to_string(), "beta".to_string()]);
    }

    #[test]
    fn run_saved_delete_removes_saved_file() {
        let _env_guard = TEST_ENV_MUTEX.lock().expect("env mutex");
        let xdg = std::env::temp_dir().join(format!(
            "na_rust_saved_delete_fixture_{}_{}",
            std::process::id(),
            FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::env::set_var("XDG_DATA_HOME", &xdg);
        let path = saved_search_path("Weekly Focus").expect("saved path should build");
        fs::create_dir_all(path.parent().expect("search dir should exist"))
            .expect("search dir should create");
        fs::write(&path, "na next").expect("saved search should write");

        run_saved(&SavedCommands::Delete {
            title: "Weekly Focus".to_string(),
        })
        .expect("delete should succeed");

        std::env::remove_var("XDG_DATA_HOME");
        fs::remove_dir_all(&xdg).ok();
        assert!(!path.exists());
    }

    #[test]
    fn run_saved_run_executes_saved_command() {
        let _env_guard = TEST_ENV_MUTEX.lock().expect("env mutex");
        let xdg = std::env::temp_dir().join(format!(
            "na_rust_saved_run_fixture_{}_{}",
            std::process::id(),
            FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&xdg).expect("xdg fixture should create");
        std::env::set_var("XDG_DATA_HOME", &xdg);
        let output = xdg.join("ran.txt");
        let path = saved_search_path("Smoke").expect("saved path should build");
        fs::create_dir_all(path.parent().expect("search dir should exist"))
            .expect("search dir should create");
        fs::write(&path, format!("echo OK > {}", output.display()))
            .expect("saved search should write");

        run_saved(&SavedCommands::Run {
            title: "Smoke".to_string(),
        })
        .expect("run should succeed");

        let ran = fs::read_to_string(&output).expect("run output should exist");
        std::env::remove_var("XDG_DATA_HOME");
        fs::remove_dir_all(&xdg).ok();
        assert_eq!(ran.trim(), "OK");
    }

    #[test]
    fn run_saved_edit_uses_editor_override() {
        let _env_guard = TEST_ENV_MUTEX.lock().expect("env mutex");
        let xdg = std::env::temp_dir().join(format!(
            "na_rust_saved_edit_fixture_{}_{}",
            std::process::id(),
            FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::env::set_var("XDG_DATA_HOME", &xdg);
        let path = saved_search_path("Edit Me").expect("saved path should build");
        fs::create_dir_all(path.parent().expect("search dir should exist"))
            .expect("search dir should create");
        fs::write(&path, "na next").expect("saved search should write");

        run_saved(&SavedCommands::Edit {
            title: "Edit Me".to_string(),
            editor: Some("true".to_string()),
        })
        .expect("edit should succeed with true");

        assert!(path.exists(), "edit should keep saved file");
        std::env::remove_var("XDG_DATA_HOME");
        fs::remove_dir_all(&xdg).ok();
    }

    #[test]
    fn run_saved_select_non_interactive_runs_first_sorted_entry() {
        let _env_guard = TEST_ENV_MUTEX.lock().expect("env mutex");
        let xdg = std::env::temp_dir().join(format!(
            "na_rust_saved_select_fixture_{}_{}",
            std::process::id(),
            FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&xdg).expect("xdg fixture should create");
        std::env::set_var("XDG_DATA_HOME", &xdg);
        let output = xdg.join("selected.txt");
        let alpha = saved_search_path("Alpha").expect("alpha path");
        let zeta = saved_search_path("Zeta").expect("zeta path");
        fs::create_dir_all(alpha.parent().expect("dir")).expect("search dir should create");
        fs::write(&alpha, format!("echo alpha > {}", output.display())).expect("alpha write");
        fs::write(&zeta, "echo zeta > /dev/null").expect("zeta write");

        run_saved(&SavedCommands::Select).expect("select should run first sorted entry");

        let ran = fs::read_to_string(&output).expect("select output should exist");
        std::env::remove_var("XDG_DATA_HOME");
        fs::remove_dir_all(&xdg).ok();
        assert_eq!(ran.trim(), "alpha");
    }

    #[test]
    fn run_tag_supports_comma_and_removal_prefixes() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Tag me @old
"#,
        );
        let cli = Cli {
            global_file: Some(path.clone()),
            no_color: true,
            ..Default::default()
        };
        let args = TagArgs {
            tags: vec!["today,!old,home".to_string()],
            query: Some("Tag me".to_string()),
            in_todo: Vec::new(),
            done: false,
            file: None,
            depth: 1,
            tagged: Vec::new(),
            regex: false,
            exact: false,
            search: Vec::new(),
            search_notes: true,
            all: true,
        };
        run_tag(&cli, &args).expect("tag should succeed");
        let updated = fs::read_to_string(&path).expect("fixture should read");
        fs::remove_file(path).ok();
        assert!(updated.contains("@today"), "{updated}");
        assert!(updated.contains("@home"), "{updated}");
        assert!(!updated.contains("@old"), "{updated}");
    }

    #[test]
    fn run_tag_done_filter_only_updates_done_actions() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Done one @done
- Open one
"#,
        );
        let cli = Cli {
            global_file: Some(path.clone()),
            no_color: true,
            ..Default::default()
        };
        let args = TagArgs {
            tags: vec!["reviewed".to_string()],
            query: None,
            in_todo: Vec::new(),
            done: true,
            file: None,
            depth: 1,
            tagged: Vec::new(),
            regex: false,
            exact: false,
            search: Vec::new(),
            search_notes: true,
            all: true,
        };
        run_tag(&cli, &args).expect("tag should succeed");
        let updated = fs::read_to_string(&path).expect("fixture should read");
        fs::remove_file(path).ok();
        assert!(updated.contains("- Done one @done @reviewed"), "{updated}");
        assert!(updated.contains("- Open one\n"), "{updated}");
    }

    #[test]
    fn run_plugin_new_creates_stub_in_plugin_dir() {
        let _env_guard = TEST_ENV_MUTEX.lock().expect("env mutex");
        let xdg = std::env::temp_dir().join(format!(
            "na_rust_plugin_new_cmd_fixture_{}_{}",
            std::process::id(),
            FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&xdg).expect("xdg fixture should create");
        std::env::set_var("XDG_DATA_HOME", &xdg);
        let cli = Cli::parse_from(["na", "plugin", "list"]);

        run_plugin(
            &cli,
            &PluginCommands::New {
                plugin: "sample".to_string(),
            },
        )
        .expect("plugin new should succeed");

        let path = xdg.join("na").join("plugins").join("sample.sh");
        let body = fs::read_to_string(&path).expect("created plugin should read");
        std::env::remove_var("XDG_DATA_HOME");
        fs::remove_dir_all(&xdg).ok();
        assert!(body.contains("na:input=json"), "{body}");
    }

    #[test]
    fn run_plugin_enable_disable_toggles_permissions() {
        let _env_guard = TEST_ENV_MUTEX.lock().expect("env mutex");
        let xdg = std::env::temp_dir().join(format!(
            "na_rust_plugin_toggle_fixture_{}_{}",
            std::process::id(),
            FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let plugin_dir = xdg.join("na").join("plugins");
        fs::create_dir_all(&plugin_dir).expect("plugin dir should create");
        let plugin = plugin_dir.join("toggle.sh");
        fs::write(&plugin, "#!/usr/bin/env bash\necho ok\n").expect("plugin should write");
        std::env::set_var("XDG_DATA_HOME", &xdg);
        let cli = Cli::parse_from(["na", "plugin", "list"]);

        run_plugin(
            &cli,
            &PluginCommands::Disable {
                plugin: plugin.display().to_string(),
            },
        )
        .expect("disable should succeed");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&plugin)
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0);
        }

        run_plugin(
            &cli,
            &PluginCommands::Enable {
                plugin: plugin.display().to_string(),
            },
        )
        .expect("enable should succeed");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&plugin)
                .expect("metadata")
                .permissions()
                .mode();
            assert_ne!(mode & 0o111, 0);
        }

        std::env::remove_var("XDG_DATA_HOME");
        fs::remove_dir_all(&xdg).ok();
    }

    #[test]
    fn run_plugin_edit_uses_editor_override() {
        let _env_guard = TEST_ENV_MUTEX.lock().expect("env mutex");
        let xdg = std::env::temp_dir().join(format!(
            "na_rust_plugin_edit_fixture_{}_{}",
            std::process::id(),
            FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let plugin_dir = xdg.join("na").join("plugins");
        fs::create_dir_all(&plugin_dir).expect("plugin dir should create");
        let plugin = plugin_dir.join("editme.sh");
        fs::write(&plugin, "#!/usr/bin/env bash\necho ok\n").expect("plugin should write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&plugin).expect("metadata").permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&plugin, perms).expect("chmod");
        }
        std::env::set_var("XDG_DATA_HOME", &xdg);
        let cli = Cli::parse_from(["na", "plugin", "list"]);
        run_plugin(
            &cli,
            &PluginCommands::Edit {
                plugin: "editme".to_string(),
                editor: Some("true".to_string()),
            },
        )
        .expect("edit should succeed");

        std::env::remove_var("XDG_DATA_HOME");
        fs::remove_dir_all(&xdg).ok();
    }

    #[test]
    fn run_plugin_run_executes_discovered_plugin() {
        let _env_guard = TEST_ENV_MUTEX.lock().expect("env mutex");
        let xdg = std::env::temp_dir().join(format!(
            "na_rust_plugin_run_fixture_{}_{}",
            std::process::id(),
            FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let plugin_dir = xdg.join("na").join("plugins");
        fs::create_dir_all(&plugin_dir).expect("plugin dir should create");
        let plugin = plugin_dir.join("echo_json.sh");
        fs::write(
            &plugin,
            "#!/usr/bin/env bash\n# na:input=json\n# na:output=text\ncat\n",
        )
        .expect("plugin should write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&plugin).expect("metadata").permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&plugin, perms).expect("chmod");
        }
        let task = write_fixture_taskpaper(
            r#"Work:
- Plugin target @na
"#,
        );
        std::env::set_var("XDG_DATA_HOME", &xdg);
        let cli = Cli {
            global_file: Some(task.clone()),
            no_color: true,
            ..Default::default()
        };
        run_plugin(
            &cli,
            &PluginCommands::Run {
                plugin: "echo_json".to_string(),
                query: "@na".to_string(),
                input: None,
                output: None,
                divider: None,
                file: None,
                depth: None,
                in_todo: Vec::new(),
                search: Vec::new(),
                done: false,
                tagged: Vec::new(),
            },
        )
        .expect("plugin run should succeed");

        std::env::remove_var("XDG_DATA_HOME");
        fs::remove_file(task).ok();
        fs::remove_dir_all(&xdg).ok();
    }

    #[test]
    fn run_archive_non_interactive_marks_undone_and_moves_to_archive() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Ship this @na
- Keep this open
"#,
        );
        let cli = Cli {
            global_file: Some(path.clone()),
            no_color: true,
            ..Default::default()
        };
        let mut args = base_archive_args();
        args.query = Some("Ship this".to_string());
        args.all = true;
        run_archive(&cli, &args).expect("archive should succeed");
        let updated = fs::read_to_string(&path).expect("fixture should read");
        fs::remove_file(path).ok();

        assert!(
            updated.contains("Archive:\n- Ship this @na @done("),
            "{updated}"
        );
        assert!(updated.contains("- Keep this open"), "{updated}");
    }

    #[test]
    fn run_archive_applies_note_and_overwrite_flags() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Ship this @na
    old note
"#,
        );
        let cli = Cli {
            global_file: Some(path.clone()),
            no_color: true,
            ..Default::default()
        };
        let mut args = base_archive_args();
        args.query = Some("Ship this".to_string());
        args.note = vec!["new archive note".to_string()];
        args.overwrite_notes = true;
        args.all = true;
        run_archive(&cli, &args).expect("archive should succeed");
        let updated = fs::read_to_string(&path).expect("fixture should read");
        fs::remove_file(path).ok();

        assert!(updated.contains("\tnew archive note"), "{updated}");
        assert!(!updated.contains("old note"), "{updated}");
    }
}
