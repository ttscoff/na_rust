use crate::cli::{
    AddArgs, ArchiveArgs, Cli, Commands, CompletedArgs, EditArgs, FindArgs, MoveArgs, NextArgs,
    OpenArgs, PluginCommands, ProjectsArgs, PromptCommands, PromptArgs, SavedCommands, ScanArgs,
    TagArgs, TaggedArgs, TodosArgs, UndoArgs, UpdateArgs,
};
use crate::io::fs::{discover_taskpaper_files, discover_taskpaper_files_with_options};
use crate::models::action::Action;
use crate::models::todo::{TodoFile, UpdateMutation};
use crate::output::formatter::{format_action, OutputStyle};
use crate::parser::search::{evaluate_query, Query};
use crate::io::xdg::na_data_dir;
use crate::plugins::format::{merge_plugin_stdout_into_actions, PluginDataFormat};
use crate::plugins::registry::{PluginRegistry, PluginRunner};
use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use inquire::{InquireError, MultiSelect, Select, Text};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

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
        Some(Commands::Prompt(args)) => run_prompt(args),
        Some(Commands::Changes) => run_changes(),
        Some(Commands::Saved(args)) => run_saved(&args.command),
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
    if let Some(title) = args.save.as_deref() {
        save_next_search(args, title)?;
    }
    let files = load_next_todo_files(cli, args)?;
    let mut display_actions = next_actions(&files, args)?;
    if args.times || args.json_times || args.only_times {
        return output_time_summary(&display_actions, args);
    }
    if let Some(merged) = run_next_plugin_merge(args, &display_actions)? {
        display_actions = merged;
    }
    let style = OutputStyle {
        color: output_color_enabled(cli),
        na_tag: "na",
        include_notes: args.effective_notes(),
    };
    let file_labels = if args.no_file {
        HashMap::new()
    } else {
        build_filename_labels(&display_actions)
    };

    if args.nest_for_display() {
        print_next_nested(args, style, &file_labels, &display_actions);
    } else {
        for action in &display_actions {
            let file_prefix = file_labels
                .get(&action.source_file)
                .map(String::as_str);
            println!("{}", format_action(action, style, file_prefix));
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
    fs::write(&path, format_saved_search(args)).with_context(|| format!("Failed to write {:?}", path))?;
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
            || matches!(c, '\'' | '"' | '\\' | '$' | '`' | '!' | '&' | '|' | ';' | '<' | '>' | '(' | ')' | '[' | ']' | '{' | '}')
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

fn print_next_nested(
    args: &NextArgs,
    style: OutputStyle,
    file_labels: &HashMap<String, String>,
    matches: &[Action],
) {
    let by_file = group_actions_by_file(matches);
    for (file, actions_in_file) in by_file {
        println!("== {file}");
        if args.omnifocus {
            let by_proj = group_actions_by_project(&actions_in_file);
            for (proj, acts) in by_proj {
                println!("## {proj}");
                for action in acts {
                    let file_prefix = file_labels
                        .get(&action.source_file)
                        .map(String::as_str);
                    println!("{}", format_action(action, style, file_prefix));
                }
            }
        } else {
            for action in actions_in_file {
                let file_prefix = file_labels
                    .get(&action.source_file)
                    .map(String::as_str);
                println!("{}", format_action(action, style, file_prefix));
            }
        }
    }
}

fn group_actions_by_file(actions: &[Action]) -> Vec<(String, Vec<&Action>)> {
    let mut out: Vec<(String, Vec<&Action>)> = Vec::new();
    for action in actions {
        if let Some(last) = out.last_mut() {
            if last.0 == action.source_file {
                last.1.push(action);
                continue;
            }
        }
        out.push((action.source_file.clone(), vec![action]));
    }
    out
}

fn group_actions_by_project<'a>(actions: &'a [&'a Action]) -> Vec<(String, Vec<&'a Action>)> {
    let mut out: Vec<(String, Vec<&Action>)> = Vec::new();
    for action in actions {
        let proj = action
            .project
            .clone()
            .unwrap_or_else(|| "(none)".to_string());
        if let Some(last) = out.last_mut() {
            if last.0 == proj {
                last.1.push(*action);
                continue;
            }
        }
        out.push((proj, vec![*action]));
    }
    out
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

fn next_actions(files: &[TodoFile], args: &NextArgs) -> Result<Vec<Action>> {
    let include_done_for_time = args.times || args.json_times || args.only_times || args.only_timed;
    let query = build_next_query(args)?.with_include_done(args.done || include_done_for_time);

    let mut matches = evaluate_query(files, &query);
    matches = apply_next_search_filter(matches, args)?;
    if args.only_timed {
        matches.retain(|a| action_elapsed_seconds(a).is_some());
    }
    if args.first_available && !next_filtered_mode(args) {
        // Ruby parity: in filtered mode we should not force @na.
        let require_na = next_requires_na(args);
        matches = crate::models::actions::first_available_per_project(matches, require_na, "na");
    }

    Ok(matches)
}

fn output_time_summary(actions: &[Action], args: &NextArgs) -> Result<()> {
    let mut timed: Vec<(String, i64)> = Vec::new();
    for action in actions {
        if let Some(secs) = action_elapsed_seconds(action) {
            timed.push((action.text.clone(), secs));
        }
    }
    let total: i64 = timed.iter().map(|(_, secs)| *secs).sum();
    if args.json_times {
        let payload = serde_json::json!({
            "total_seconds": total,
            "actions": timed.iter().map(|(text, secs)| serde_json::json!({"text": text, "seconds": secs})).collect::<Vec<_>>()
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }
    if args.only_times {
        if args.human {
            println!("{}", human_duration(total));
        } else {
            println!("{total}");
        }
        return Ok(());
    }
    for (text, secs) in &timed {
        if args.human {
            println!("{text} [{}]", human_duration(*secs));
        } else {
            println!("{text} [{secs}s]");
        }
    }
    if args.human {
        println!("Total: {}", human_duration(total));
    } else {
        println!("Total: {total}s");
    }
    Ok(())
}

fn human_duration(total_seconds: i64) -> String {
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

fn action_elapsed_seconds(action: &Action) -> Option<i64> {
    let started = parse_tag_datetime(action.tag_value("started")?)?;
    let done = parse_tag_datetime(action.tag_value("done")?)?;
    let secs = (done - started).num_seconds();
    if secs >= 0 { Some(secs) } else { None }
}

fn parse_tag_datetime(input: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(input) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(input, "%Y-%m-%d %H:%M:%S") {
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc));
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(input, "%Y-%m-%d %H:%M") {
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc));
    }
    if let Ok(nd) = NaiveDate::parse_from_str(input, "%Y-%m-%d") {
        let ndt = nd.and_hms_opt(0, 0, 0)?;
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc));
    }
    None
}

fn build_next_query(args: &NextArgs) -> Result<Query> {
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
        return Ok(Query::next_defaults("na"));
    }

    let na_tag = args.tag.as_deref().unwrap_or("na").trim_start_matches('@');
    let mut predicates: Vec<String> = Vec::new();
    if next_requires_na(args) {
        predicates.push(format!("@{na_tag}"));
    }
    let done_enabled = args.done || args.times || args.only_timed || args.json_times || args.only_times;
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
        || args.filter.is_some()
        || args.tag.is_some()
        || args.project.is_some()
        || !args.tagged.is_empty()
        || !args.priority.is_empty()
        || !args.search.is_empty()
        || args.save.is_some()
        || args.plugin.is_some()
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
    if let Some(title) = args.save.as_deref() {
        let mut next_args = implicit_next_args();
        next_args.filter = Some(args.query.clone());
        save_next_search(&next_args, title)?;
    }
    let files = load_find_todo_files(cli, args)?;
    let mut matches = find_actions(&files, &args.query)?;
    if !args.done {
        matches.retain(|a| !a.done);
    }
    if let Some(project) = &args.project {
        let needle = project.to_ascii_lowercase();
        matches.retain(|a| a.project_chain.iter().any(|p| p.to_ascii_lowercase().contains(&needle)));
    }
    if !args.tagged.is_empty() {
        matches.retain(|a| {
            args.tagged.iter().all(|t| {
                let tag = if t.starts_with('@') { t.to_ascii_lowercase() } else { format!("@{}", t.to_ascii_lowercase()) };
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

    let style = OutputStyle {
        color: output_color_enabled(cli),
        na_tag: "na",
        include_notes: args.effective_notes(),
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
            style,
            &file_labels,
            &matches,
        );
    } else {
        for action in matches {
            let file_prefix = file_labels.get(&action.source_file).map(String::as_str);
            println!("{}", format_action(&action, style, file_prefix));
        }
    }
    Ok(())
}

fn load_find_todo_files(cli: &Cli, args: &FindArgs) -> Result<Vec<TodoFile>> {
    let depth = args.depth.unwrap_or(5);
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

fn run_tagged(cli: &Cli, args: &TaggedArgs) -> Result<()> {
    let mut find = args.find.clone();
    if !args.tags.is_empty() {
        find.query = args
            .tags
            .iter()
            .map(|t| if t.starts_with('@') { t.clone() } else { format!("@{t}") })
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
        matches.retain(|a| completed_matches_pattern(a, &args.pattern, args.effective_search_notes()));
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
        na_tag: "na",
        include_notes: args.effective_notes(),
    };
    let file_labels = build_filename_labels(&matches);
    if args.nest_for_display() {
        let by_file = group_actions_by_file(&matches);
        for (file, actions_in_file) in by_file {
            println!("== {file}");
            if args.omnifocus {
                let by_proj = group_actions_by_project(&actions_in_file);
                for (proj, acts) in by_proj {
                    println!("## {proj}");
                    for action in acts {
                        let file_prefix = file_labels.get(&action.source_file).map(String::as_str);
                        println!("{}", format_action(action, style, file_prefix));
                    }
                }
            } else {
                for action in actions_in_file {
                    let file_prefix = file_labels.get(&action.source_file).map(String::as_str);
                    println!("{}", format_action(action, style, file_prefix));
                }
            }
        }
    } else {
        for action in matches {
            let file_prefix = file_labels.get(&action.source_file).map(String::as_str);
            println!("{}", format_action(&action, style, file_prefix));
        }
    }
    Ok(())
}

fn load_completed_todo_files(cli: &Cli, args: &CompletedArgs) -> Result<Vec<TodoFile>> {
    let depth = args.depth.unwrap_or(5);
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
    let target = files
        .get_mut(file_idx)
        .context("No TaskPaper file found")?;
    let project = resolve_add_project(target, &args.project, interactive)?;
    let action_text = build_add_text(args);
    let notes = collect_add_notes(args, &args.text)?;
    let append = add_position_is_append(args.at.as_deref());
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
    let labels: Vec<String> = files
        .iter()
        .map(|f| f.path.display().to_string())
        .collect();
    choose_from_menu("Multiple todo files found, select target", &labels, interactive)
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
    let mut files = discover_taskpaper_files_with_options(&cli.extension, args.depth, false)?;
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
    let idx = choose_from_menu("Multiple matching projects found, select target", &resolved, interactive)?;
    Ok(resolved[idx].clone())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathAxis {
    Child,
    Desc,
}

#[derive(Debug, Clone)]
struct PathStep {
    axis: PathAxis,
    text: String,
    wildcard: bool,
}

fn resolve_item_path(path: &str, projects: &[String]) -> Vec<String> {
    let steps = parse_item_path(path);
    if steps.is_empty() {
        return Vec::new();
    }
    let chains: Vec<Vec<String>> = projects
        .iter()
        .map(|p| p.split(':').map(|s| s.trim().to_string()).collect::<Vec<_>>())
        .collect();
    let mut current: Vec<Vec<String>> = vec![Vec::new()];
    for step in steps {
        let mut next = Vec::<Vec<String>>::new();
        match step.axis {
            PathAxis::Child => {
                for base in &current {
                    for chain in &chains {
                        if chain.len() <= base.len() {
                            continue;
                        }
                        if !chain.starts_with(base) {
                            continue;
                        }
                        let part = &chain[base.len()];
                        if step.wildcard || part.eq_ignore_ascii_case(&step.text) {
                            let candidate = chain[..=base.len()].to_vec();
                            if !next.iter().any(|n| n == &candidate) {
                                next.push(candidate);
                            }
                        }
                    }
                }
            }
            PathAxis::Desc => {
                for base in &current {
                    for chain in &chains {
                        if chain.len() <= base.len() || !chain.starts_with(base) {
                            continue;
                        }
                        for i in base.len()..chain.len() {
                            let part = &chain[i];
                            if step.wildcard || part.eq_ignore_ascii_case(&step.text) {
                                let candidate = chain[..=i].to_vec();
                                if !next.iter().any(|n| n == &candidate) {
                                    next.push(candidate);
                                }
                            }
                        }
                    }
                }
            }
        }
        current = next;
        if current.is_empty() {
            break;
        }
    }
    current
        .into_iter()
        .map(|c| c.join(":"))
        .collect()
}

fn parse_item_path(path: &str) -> Vec<PathStep> {
    let s = path.trim();
    if !s.starts_with('/') {
        return Vec::new();
    }
    let bytes = s.as_bytes();
    let mut i = 0usize;
    let mut out = Vec::new();
    while i < bytes.len() {
        if bytes[i] != b'/' {
            break;
        }
        let axis = if i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            i += 2;
            PathAxis::Desc
        } else {
            i += 1;
            PathAxis::Child
        };
        let start = i;
        while i < bytes.len() && bytes[i] != b'/' {
            i += 1;
        }
        let text = s[start..i].trim().to_string();
        if text.is_empty() {
            continue;
        }
        out.push(PathStep {
            axis,
            wildcard: text == "*",
            text,
        });
    }
    out
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

fn build_add_text(args: &AddArgs) -> String {
    let mut action = strip_trailing_note(&args.text).0;

    if let Some(priority) = parse_priority_value(args.priority.as_deref()) {
        action = remove_tag_value(&action, "priority");
        action.push_str(&format!(" @priority({priority})"));
    }

    if !args.no_next_tag {
        let next_tag = args.tag.as_deref().unwrap_or("na");
        action = remove_tag_value(&action, next_tag);
        action.push_str(&format!(" @{}", next_tag.trim_start_matches('@')));
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
    let mut files = load_update_todo_files(cli, args)?;
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let matches = collect_update_candidates(&files, args)?;
    if matches.is_empty() {
        anyhow::bail!("No matching actions found");
    }

    let mut add_tags = args.tag.clone();
    let mut remove_tags = args.untag.clone();
    let mut mark_done = args.done;

    let needs_action_menu = args.query.is_none()
        && args.search.is_empty()
        && add_tags.is_empty()
        && remove_tags.is_empty()
        && !mark_done;

    let selected = if needs_action_menu {
        let chosen = choose_actions_interactive(&matches, interactive)?;
        let op = prompt_update_operation(interactive)?;
        if op.cancelled {
            anyhow::bail!("Update cancelled");
        }
        add_tags = op.add_tags;
        remove_tags = op.remove_tags;
        mark_done = op.done;
        chosen
    } else if args.all || !interactive {
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

    let mutation = UpdateMutation {
        add_tags: add_tags.clone(),
        remove_tags: remove_tags.clone(),
        done: mark_done,
        replace_text: args.replace.clone(),
        move_to_project: args.to.clone(),
        delete: args.delete,
        restore: args.restore,
        note_lines: args.note.clone(),
        overwrite_notes: args.overwrite_notes,
    };
    let mut updated_count = 0usize;
    for todo in &mut files {
        let key = todo.path.display().to_string();
        if let Some(lines) = by_file.get(&key) {
            updated_count += todo.apply_mutation_by_lines(lines, &mutation)?;
        }
    }
    println!("Updated {updated_count} action(s)");
    Ok(())
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
        delete: false,
        restore: false,
        note: Vec::new(),
        overwrite_notes: false,
        started: None,
        end: None,
        duration: None,
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
        delete: false,
        restore: false,
        note: Vec::new(),
        overwrite_notes: false,
        started: None,
        end: None,
        duration: None,
    };
    run_update(cli, &update)
}

fn run_tag(cli: &Cli, args: &TagArgs) -> Result<()> {
    let mut add = Vec::new();
    let mut remove = Vec::new();
    for t in &args.tags {
        if let Some(stripped) = t.strip_prefix('!').or_else(|| t.strip_prefix('-')) {
            remove.push(if stripped.starts_with('@') { stripped.to_string() } else { format!("@{stripped}") });
        } else {
            add.push(if t.starts_with('@') { t.clone() } else { format!("@{t}") });
        }
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
        tagged: args.tagged.clone(),
        regex: args.regex,
        exact: args.exact,
        search_notes: args.search_notes,
        no_search_notes: !args.search_notes,
        priority: None,
        at: None,
        archive: false,
        edit: false,
        delete: false,
        restore: false,
        note: Vec::new(),
        overwrite_notes: false,
        started: None,
        end: None,
        duration: None,
    };
    run_update(cli, &update)
}

fn run_open(cli: &Cli, args: &OpenArgs) -> Result<()> {
    let mut paths = if let Some(path) = &cli.global_file {
        vec![path.clone()]
    } else {
        discover_taskpaper_files_with_options(&cli.extension, args.depth, false)?
    };
    if !args.in_todo.is_empty() {
        let specs = parse_todo_specs(&args.in_todo);
        paths.retain(|p| match_todo_path(p.to_string_lossy().as_ref(), &specs));
    }
    let Some(path) = paths.first() else { anyhow::bail!("No todo file found"); };
    let editor = args
        .editor
        .clone()
        .or_else(|| std::env::var("EDITOR").ok())
        .unwrap_or_else(|| "vi".to_string());
    let status = std::process::Command::new(editor).arg(path).status()?;
    if !status.success() {
        anyhow::bail!("Open command failed");
    }
    Ok(())
}

fn run_projects(cli: &Cli, args: &ProjectsArgs) -> Result<()> {
    let files = if let Some(path) = &cli.global_file {
        vec![TodoFile::load(path)?]
    } else {
        discover_taskpaper_files_with_options(&cli.extension, args.depth, false)?
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
        let Some(first) = paths.first() else { return Ok(()); };
        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
        let _ = std::process::Command::new(editor).arg(first).status()?;
    }
    Ok(())
}

fn run_undo(_cli: &Cli, _args: &UndoArgs) -> Result<()> {
    anyhow::bail!("undo command not yet implemented")
}

fn run_scan(cli: &Cli, args: &ScanArgs) -> Result<()> {
    let files = discover_taskpaper_files_with_options(&cli.extension, args.depth, args.hidden)?;
    for f in files {
        println!("{}", f.display());
    }
    if args.prune || args.dry_run {
        let _ = (args.prune, args.dry_run);
    }
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

fn run_prompt(args: &PromptArgs) -> Result<()> {
    match args.command.clone().unwrap_or(PromptCommands::Show) {
        PromptCommands::Show => {
            println!("# Add this to your shell profile");
            println!("eval \"$(na prompt show)\"");
            Ok(())
        }
        PromptCommands::Install => {
            println!("Prompt installation is not yet automated in Rust version.");
            Ok(())
        }
    }
}

fn run_changes() -> Result<()> {
    println!("See CHANGELOG.md for recent changes.");
    Ok(())
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
            moved_count += todo.archive_actions_by_lines(lines)?;
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
    let mut paths = discover_taskpaper_files_with_options(&cli.extension, args.depth, false)?;
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
    let mut paths = discover_taskpaper_files_with_options(&cli.extension, args.depth, false)?;
    if !args.in_todo.is_empty() {
        let specs = parse_todo_specs(&args.in_todo);
        paths.retain(|path| match_todo_path(path.to_string_lossy().as_ref(), &specs));
    }
    paths
        .iter()
        .map(|path| TodoFile::load(path).with_context(|| format!("Failed to read {:?}", path)))
        .collect()
}

fn collect_update_candidates(files: &[TodoFile], args: &UpdateArgs) -> Result<Vec<Action>> {
    let mut out = if let Some(q) = args.query.as_deref() {
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
        out.retain(|a| a.project_chain.iter().any(|p| p.to_ascii_lowercase().contains(&needle)));
    }
    if !args.tagged.is_empty() {
        out.retain(|a| {
            args.tagged.iter().all(|t| {
                let tag = if t.starts_with('@') { t.to_ascii_lowercase() } else { format!("@{}", t.to_ascii_lowercase()) };
                a.tags.iter().any(|x| x.eq_ignore_ascii_case(&tag))
            })
        });
    }
    Ok(out)
}

#[derive(Debug, Default, Clone)]
struct UpdateMenuSelection {
    add_tags: Vec<String>,
    remove_tags: Vec<String>,
    done: bool,
    cancelled: bool,
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

fn prompt_update_operation(interactive: bool) -> Result<UpdateMenuSelection> {
    if !interactive {
        return Ok(UpdateMenuSelection::default());
    }
    let ops = vec![
        "Add Tag".to_string(),
        "Remove Tag".to_string(),
        "Finish (mark done)".to_string(),
        "Cancel".to_string(),
    ];
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
        "Add Tag" => {
            let input = Text::new("Tag(s) to add (comma-separated)")
                .prompt()
                .map_err(map_inquire_error)?;
            Ok(UpdateMenuSelection {
                add_tags: parse_tag_input(&input),
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
        "Finish (mark done)" => Ok(UpdateMenuSelection {
            done: true,
            ..UpdateMenuSelection::default()
        }),
        "Cancel" => Ok(UpdateMenuSelection {
            cancelled: true,
            ..UpdateMenuSelection::default()
        }),
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
        .map(|s| {
            if s.starts_with('@') {
                s.to_string()
            } else {
                format!("@{s}")
            }
        })
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
            for name in list_saved_searches()? {
                println!("{name}");
            }
            Ok(())
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
                    depth.unwrap_or(5),
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
        abbreviate_source_path, action_elapsed_seconds, add_position_is_append, build_add_text,
        choose_from_menu, completed_matches_date, completed_matches_pattern, find_actions,
        format_saved_search, implicit_next_args, is_cancel_selection, load_next_todo_files,
        list_saved_searches, load_update_todo_files, match_todo_path, next_actions, parse_multi_selection,
        parse_one_based_selection, parse_priority_value, parse_tag_input, parse_tag_datetime,
        parse_todo_specs, resolve_item_path, run_archive, run_edit, run_plugin, run_saved,
        run_update, save_next_search, saved_search_path, saved_search_slug, shell_quote_token,
        strip_trailing_note,
    };
    use crate::cli::{
        AddArgs, ArchiveArgs, Cli, Commands, CompletedArgs, EditArgs, NextArgs, PluginCommands,
        SavedCommands, UpdateArgs,
    };
    use crate::io::xdg::TEST_ENV_MUTEX;
    use crate::models::action::Action;
    use crate::models::todo::TodoFile;
    use clap::Parser;
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
        let out = next_actions(&[todo], &args).expect("next should evaluate");
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
        let out = next_actions(&[todo], &args).expect("next should evaluate");
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
        args.filter = Some(r#"@search(@context contains "home")"#.to_string());
        args.first_available = true;
        let out = next_actions(&[todo], &args).expect("next should evaluate");
        fs::remove_file(path).ok();

        assert_eq!(out.len(), 3);
        assert!(out[0].text.contains("First A no tag"));
        assert!(out[1].text.contains("Second A"));
        assert!(out[2].text.contains("First B no tag"));
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
        let out = next_actions(&[todo], &args).expect("next should evaluate");
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
        let out = next_actions(&[todo], &args).expect("next should evaluate");
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
        let out = next_actions(&[todo], &args).expect("next should evaluate");
        fs::remove_file(path).ok();
        assert!(out.is_empty());
    }

    #[test]
    fn saved_search_slug_normalizes_and_collapses_separators() {
        assert_eq!(saved_search_slug("  Weekly Focus / Work  "), "weekly_focus_work");
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
        assert!(formatted.starts_with("na next --first-available"), "{formatted}");
        assert!(formatted.contains("--project 'Client Alpha'"), "{formatted}");
        assert!(formatted.contains("--search 'one two'"), "{formatted}");
        assert!(formatted.contains("--file '/tmp/with space.taskpaper'"), "{formatted}");
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
        let out = next_actions(&[todo], &args).expect("next should evaluate");
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
            depth: 1,
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
            delete: false,
            restore: false,
            note: Vec::new(),
            overwrite_notes: false,
            started: None,
            end: None,
            duration: None,
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
        let text = build_add_text(&args);
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
        assert!(!match_todo_path("/tmp/work-client-archive.taskpaper", &specs));
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
    fn run_update_non_interactive_query_branch_updates_matching_actions() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Match me
- Skip me
"#,
        );
        let cli = Cli {
            version: false,
            extension: "taskpaper".to_string(),
            global_file: Some(path.clone()),
            no_color: true,
            command: None,
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
            version: false,
            extension: "taskpaper".to_string(),
            global_file: Some(path.clone()),
            no_color: true,
            command: None,
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
            version: false,
            extension: "taskpaper".to_string(),
            global_file: Some(path.clone()),
            no_color: true,
            command: None,
        };
        let mut args = base_update_args();
        args.tag = vec!["@bulk".to_string()];

        run_update(&cli, &args).expect("run_update should succeed");
        let updated = fs::read_to_string(&path).expect("fixture should read");
        fs::remove_file(path).ok();

        assert!(updated.contains("- First @bulk"), "{updated}");
        assert!(updated.contains("- Second @bulk"), "{updated}");
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
            version: false,
            extension: "taskpaper".to_string(),
            global_file: Some(path.clone()),
            no_color: true,
            command: None,
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
            version: false,
            extension: "taskpaper".to_string(),
            global_file: Some(path.clone()),
            no_color: true,
            command: None,
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
            version: false,
            extension: "taskpaper".to_string(),
            global_file: Some(path.clone()),
            no_color: true,
            command: None,
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
            version: false,
            extension: "taskpaper".to_string(),
            global_file: Some(path.clone()),
            no_color: true,
            command: None,
        };
        let mut args = base_update_args();
        args.query = Some("Move me".to_string());
        args.to = Some("Inbox".to_string());
        run_update(&cli, &args).expect("run_update should succeed");
        let updated = fs::read_to_string(&path).expect("fixture should read");
        fs::remove_file(path).ok();
        assert!(updated.contains("Inbox:\n- Keep here\n- Move me @na"), "{updated}");
    }

    #[test]
    fn load_update_todo_files_prefers_explicit_file_over_global_or_discovery() {
        let explicit = write_named_fixture_taskpaper("explicit", "Inbox:\n- Explicit\n");
        let global = write_named_fixture_taskpaper("global", "Inbox:\n- Global\n");
        let cli = Cli {
            version: false,
            extension: "taskpaper".to_string(),
            global_file: Some(global.clone()),
            no_color: true,
            command: None,
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
            version: false,
            extension: "taskpaper".to_string(),
            global_file: None,
            no_color: true,
            command: None,
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
        assert!(loaded_paths.iter().any(|p| p.contains("keep_work_client")), "{loaded_paths:?}");
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
        let mut actions: Vec<Action> = vec![todo.actions()[0].clone(), todo.actions()[1].clone(), todo.actions()[2].clone()];
        actions.retain(|a| a.done);
        actions.retain(|a| completed_matches_pattern(a, &args.pattern, args.effective_search_notes()));
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
        fs::write(&path, format!("echo OK > {}", output.display())).expect("saved search should write");

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
            let mode = fs::metadata(&plugin).expect("metadata").permissions().mode();
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
            let mode = fs::metadata(&plugin).expect("metadata").permissions().mode();
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
            version: false,
            extension: "taskpaper".to_string(),
            global_file: Some(task.clone()),
            no_color: true,
            command: None,
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
            version: false,
            extension: "taskpaper".to_string(),
            global_file: Some(path.clone()),
            no_color: true,
            command: None,
        };
        let mut args = base_archive_args();
        args.query = Some("Ship this".to_string());
        args.all = true;
        run_archive(&cli, &args).expect("archive should succeed");
        let updated = fs::read_to_string(&path).expect("fixture should read");
        fs::remove_file(path).ok();

        assert!(updated.contains("Archive:\n- Ship this @na @done("), "{updated}");
        assert!(updated.contains("- Keep this open"), "{updated}");
    }
}
