use crate::models::action::Action;
use crate::models::todo::{TodoFile, UpdateMutation};
use crate::plugins::format::{
    merge_plugin_stdout_into_actions, merge_text_with_plugin_tag_objects, paths_match,
    PluginDataFormat,
};
use crate::parser::expand_date_tags_in_line;
use crate::parser::taskpaper::refresh_action_tags;
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginCommandKind {
    Update,
    Delete,
    Complete,
    Restore,
    Archive,
    AddTag,
    RemoveTag,
    Move,
}

#[derive(Debug, Clone)]
pub struct PluginPersistRow {
    pub source_file: String,
    pub line_index: usize,
    pub command: PluginCommandKind,
    pub arguments: Vec<String>,
    pub text: Option<String>,
    pub note: Option<String>,
    pub parents: Vec<String>,
    pub tags_json: Option<Vec<Value>>,
}

/// Run plugin stdout against `files`, applying ACTION rows when present (Ruby `apply_plugin_result`).
/// Falls back to merge-and-replace when rows are plain UPDATE payloads.
pub fn apply_plugin_stdout_to_files(
    files: &mut [TodoFile],
    selected: &[Action],
    stdout: &str,
    format: PluginDataFormat,
    divider: Option<&str>,
) -> Result<usize> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(0);
    }

    let rows = parse_plugin_persist_rows(trimmed, format, divider).unwrap_or_default();
    if rows.is_empty() {
        return apply_merged_plugin_update(files, selected, trimmed, format, divider);
    }

    let mut total = 0usize;
    let mut sorted = rows;
    sorted.sort_by(|a, b| b.line_index.cmp(&a.line_index));
    for row in sorted {
        let Some(todo) = files
            .iter_mut()
            .find(|f| paths_match(&f.path.display().to_string(), &row.source_file))
        else {
            continue;
        };
        if apply_plugin_persist_row(todo, &row)? {
            total += 1;
        }
    }
    Ok(total)
}

fn apply_merged_plugin_update(
    files: &mut [TodoFile],
    selected: &[Action],
    stdout: &str,
    format: PluginDataFormat,
    divider: Option<&str>,
) -> Result<usize> {
    let merged = merge_plugin_stdout_into_actions(selected, stdout, format, divider)?;
    let mut total = 0usize;
    for todo in files.iter_mut() {
        let mut updates: Vec<(usize, Action)> = Vec::new();
        for (before, after) in selected.iter().zip(merged.iter()) {
            if !paths_match(&todo.path.display().to_string(), &before.source_file) {
                continue;
            }
            updates.push((before.line_index, after.clone()));
        }
        if !updates.is_empty() {
            total += todo.replace_action_blocks_from_plugin(&updates)?;
        }
    }
    Ok(total)
}

fn apply_plugin_persist_row(todo: &mut TodoFile, row: &PluginPersistRow) -> Result<bool> {
    let lines = HashSet::from([row.line_index]);
    let changed = match row.command {
        PluginCommandKind::Delete => {
            todo.apply_mutation_by_lines(&lines, &UpdateMutation {
                delete: true,
                ..UpdateMutation::default()
            })?
        }
        PluginCommandKind::Complete => {
            todo.apply_mutation_by_lines(&lines, &UpdateMutation {
                done: true,
                ..UpdateMutation::default()
            })?
        }
        PluginCommandKind::Restore => {
            todo.apply_mutation_by_lines(&lines, &UpdateMutation {
                restore: true,
                ..UpdateMutation::default()
            })?
        }
        PluginCommandKind::Archive => {
            todo.archive_actions_by_lines_with_options(&lines, &[], false)?
        }
        PluginCommandKind::AddTag => {
            let add_tags: Vec<String> = row.arguments.iter().map(|t| ensure_at_tag(t)).collect();
            todo.apply_mutation_by_lines(
                &lines,
                &UpdateMutation {
                    add_tags,
                    ..UpdateMutation::default()
                },
            )?
        }
        PluginCommandKind::RemoveTag => {
            let remove_tags: Vec<String> = row.arguments.iter().map(|t| ensure_at_tag(t)).collect();
            todo.apply_mutation_by_lines(
                &lines,
                &UpdateMutation {
                    remove_tags,
                    ..UpdateMutation::default()
                },
            )?
        }
        PluginCommandKind::Move => {
            let Some(target) = row.arguments.first() else {
                return Ok(false);
            };
            todo.apply_mutation_by_lines(
                &lines,
                &UpdateMutation {
                    move_to_project: Some(target.clone()),
                    append_to_project_end: false,
                    ..UpdateMutation::default()
                },
            )?
        }
        PluginCommandKind::Update => {
            let Some(cur) = todo
                .actions()
                .into_iter()
                .find(|a| a.line_index == row.line_index)
            else {
                return Ok(false);
            };
            let mut merged = cur.clone();
            if let Some(text) = &row.text {
                if let Some(tags) = &row.tags_json {
                    merged.text =
                        merge_text_with_plugin_tag_objects(text, tags).unwrap_or_else(|_| text.clone());
                } else {
                    merged.text = text.clone();
                }
                refresh_action_tags(&mut merged);
            } else if let Some(tags) = &row.tags_json {
                merged.text =
                    merge_text_with_plugin_tag_objects(&merged.text, tags).unwrap_or(merged.text.clone());
                refresh_action_tags(&mut merged);
            }
            if let Some(note) = &row.note {
                merged.notes = if note.is_empty() {
                    Vec::new()
                } else {
                    note.lines().map(str::to_string).collect()
                };
            }
            if !row.parents.is_empty() {
                merged.project_chain = row.parents.clone();
                merged.project = merged.project_chain.last().cloned();
            }
            merged.text = expand_date_tags_in_line(&merged.text);
            let move_to = if !row.parents.is_empty() {
                let new_proj = row.parents.join(":");
                let old = cur
                    .project_chain
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(":");
                if new_proj != old {
                    Some(new_proj)
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(to) = move_to {
                todo.apply_mutation_by_lines(
                    &lines,
                    &UpdateMutation {
                        replace_text: Some(merged.text),
                        note_lines: merged.notes.clone(),
                        overwrite_notes: true,
                        move_to_project: Some(to),
                        append_to_project_end: false,
                        ..UpdateMutation::default()
                    },
                )?
            } else {
                todo.replace_action_blocks_from_plugin(&[(row.line_index, merged)])?
            }
        }
    };
    Ok(changed > 0)
}

fn parse_plugin_persist_rows(
    stdout: &str,
    format: PluginDataFormat,
    divider: Option<&str>,
) -> Result<Vec<PluginPersistRow>> {
    match format {
        PluginDataFormat::Json | PluginDataFormat::Yaml => parse_json_rows(stdout),
        PluginDataFormat::Csv => Ok(Vec::new()),
        PluginDataFormat::TextDivider => {
            let div = divider.unwrap_or("||");
            Ok(parse_text_rows(stdout, div))
        }
    }
}

fn parse_json_rows(stdout: &str) -> Result<Vec<PluginPersistRow>> {
    let arr: Vec<Value> = serde_json::from_str(stdout).context("plugin JSON array")?;
    Ok(arr.iter().filter_map(row_from_json_value).collect())
}

fn row_from_json_value(v: &Value) -> Option<PluginPersistRow> {
    let source_file = v
        .get("source_file")
        .or_else(|| v.get("file_path"))
        .and_then(|x| x.as_str())?
        .to_string();
    let line_index = resolve_line_index(v)?;
    let (command, arguments) = parse_command_block(v.get("action"));
    let text = v
        .get("text")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let note = v
        .get("note")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let parents = v
        .get("parents")
        .or_else(|| v.get("project_chain"))
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let tags_json = v.get("tags").and_then(|x| x.as_array()).cloned();
    Some(PluginPersistRow {
        source_file,
        line_index,
        command,
        arguments,
        text,
        note,
        parents,
        tags_json,
    })
}

fn parse_text_rows(stdout: &str, divider: &str) -> Vec<PluginPersistRow> {
    stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| parse_text_row(line, divider))
        .collect()
}

fn parse_text_row(line: &str, divider: &str) -> Option<PluginPersistRow> {
    let tokens: Vec<&str> = line.splitn(8, divider).collect();
    let (command, arguments, fileline, parents, text, note, tags_raw) =
        if tokens.first().is_some_and(|t| is_action_name(t)) {
            let cmd = tokens.first()?.trim();
            let args = tokens.get(1).copied().unwrap_or("").trim();
            (
                cmd,
                args,
                tokens.get(2).copied().unwrap_or("").trim(),
                tokens.get(3).copied().unwrap_or("").trim(),
                tokens.get(4).copied(),
                tokens.get(5).copied(),
                tokens.get(6).copied(),
            )
        } else {
            (
                "UPDATE",
                "",
                tokens.first().copied().unwrap_or("").trim(),
                tokens.get(1).copied().unwrap_or("").trim(),
                tokens.get(2).copied(),
                tokens.get(3).copied(),
                tokens.get(4).copied(),
            )
        };
    let (file, line_num) = fileline.split_once(':')?;
    let line_1: usize = line_num.parse().ok()?;
    if line_1 == 0 {
        return None;
    }
    let (command, arguments) = normalize_command(command, arguments);
    Some(PluginPersistRow {
        source_file: file.to_string(),
        line_index: line_1 - 1,
        command,
        arguments,
        text: text.map(|s| s.replace("\\n", "\n")),
        note: note.map(|s| s.replace("\\n", "\n")),
        parents: if parents.is_empty() {
            Vec::new()
        } else {
            parents.split('>').map(str::trim).map(str::to_string).collect()
        },
        tags_json: tags_raw.map(parse_tags_semicolon),
    })
}

fn parse_tags_semicolon(raw: &str) -> Vec<Value> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    raw.split(';')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            if let Some((name, value)) = part.split_once('(').and_then(|(n, rest)| {
                rest.strip_suffix(')').map(|v| (n, v))
            }) {
                Some(serde_json::json!({"name": name.trim(), "value": value}))
            } else {
                Some(serde_json::json!({"name": part, "value": ""}))
            }
        })
        .collect()
}

fn parse_command_block(v: Option<&Value>) -> (PluginCommandKind, Vec<String>) {
    match v {
        Some(Value::Object(obj)) => {
            let name = obj
                .get("action")
                .and_then(|x| x.as_str())
                .unwrap_or("UPDATE");
            let args = obj
                .get("arguments")
                .and_then(|x| x.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            normalize_command(name, &args.join(","))
        }
        Some(Value::String(s)) => normalize_command(s, ""),
        None => (PluginCommandKind::Update, Vec::new()),
        Some(_) => (PluginCommandKind::Update, Vec::new()),
    }
}

fn normalize_command(name: &str, args: &str) -> (PluginCommandKind, Vec<String>) {
    let upper = name.trim().to_ascii_uppercase();
    let upper = match upper.as_str() {
        "FINISH" => "COMPLETE".to_string(),
        "UNFINISH" => "RESTORE".to_string(),
        "REMOVE_TAG" => "DELETE_TAG".to_string(),
        other => other.to_string(),
    };
    let arguments: Vec<String> = if args.trim().is_empty() {
        Vec::new()
    } else {
        args.split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    };
    let command = match upper.as_str() {
        "DELETE" => PluginCommandKind::Delete,
        "COMPLETE" => PluginCommandKind::Complete,
        "RESTORE" => PluginCommandKind::Restore,
        "ARCHIVE" => PluginCommandKind::Archive,
        "ADD_TAG" => PluginCommandKind::AddTag,
        "DELETE_TAG" => PluginCommandKind::RemoveTag,
        "MOVE" => PluginCommandKind::Move,
        _ => PluginCommandKind::Update,
    };
    (command, arguments)
}

fn is_action_name(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "update"
            | "delete"
            | "complete"
            | "finish"
            | "restore"
            | "unfinish"
            | "archive"
            | "add_tag"
            | "delete_tag"
            | "remove_tag"
            | "move"
    )
}

fn resolve_line_index(v: &Value) -> Option<usize> {
    if let Some(i) = v.get("line_index").and_then(|x| x.as_u64()) {
        return Some(i as usize);
    }
    if let Some(i) = v.get("line").and_then(|x| x.as_u64()) {
        if i == 0 {
            return None;
        }
        return Some(i as usize - 1);
    }
    None
}

fn ensure_at_tag(raw: &str) -> String {
    let s = raw.trim();
    if s.starts_with('@') {
        s.to_string()
    } else {
        format!("@{s}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::todo::TodoFile;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn write_tp(content: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "na_plugin_apply_{}_{}.taskpaper",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&path, content).expect("write");
        path
    }

    #[test]
    fn apply_add_tag_json_action() {
        let path = write_tp(
            "Work:\n\t- Ship @na\n",
        );
        let file = path.display().to_string();
        let mut todo = TodoFile::load(&path).expect("load");
        let selected = todo.actions();
        let stdout = format!(
            r#"[{{"action":{{"action":"ADD_TAG","arguments":["today"]}},"file_path":"{}","line":2}}]"#,
            file.replace('\\', "\\\\")
        );
        let n = apply_plugin_stdout_to_files(
            std::slice::from_mut(&mut todo),
            &selected,
            &stdout,
            PluginDataFormat::Json,
            None,
        )
        .expect("apply");
        assert_eq!(n, 1);
        let body = fs::read_to_string(&path).expect("read");
        assert!(body.contains("@today"), "{body}");
        fs::remove_file(path).ok();
    }

    #[test]
    fn parse_text_row_add_tag() {
        let row = parse_text_row("ADD_TAG||today||tasks.taskpaper:2||Work||Body||note||na", "||")
            .expect("row");
        assert_eq!(row.command, PluginCommandKind::AddTag);
        assert_eq!(row.arguments, vec!["today"]);
        assert_eq!(row.line_index, 1);
    }
}
