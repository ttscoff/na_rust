use crate::models::action::Action;
use crate::parser::taskpaper::refresh_action_tags;
use anyhow::{Context, Result};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginDataFormat {
    Json,
    Yaml,
    Csv,
    TextDivider,
}

impl PluginDataFormat {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "json" => Some(Self::Json),
            "yaml" | "yml" => Some(Self::Yaml),
            "csv" => Some(Self::Csv),
            "text" | "divider" | "text-divider" => Some(Self::TextDivider),
            _ => None,
        }
    }
}

#[cfg(test)]
pub fn serialize_actions(actions: &[Action], format: PluginDataFormat) -> Result<String> {
    serialize_actions_with_divider(actions, format, None)
}

/// Serialize for plugin stdin. For [`PluginDataFormat::TextDivider`], `divider` separates blocks
/// (default `---`).
pub fn serialize_actions_with_divider(
    actions: &[Action],
    format: PluginDataFormat,
    divider: Option<&str>,
) -> Result<String> {
    match format {
        PluginDataFormat::Json => Ok(serde_json::to_string(actions)?),
        PluginDataFormat::Yaml => Ok(to_yaml(actions)),
        PluginDataFormat::Csv => Ok(to_csv(actions)),
        PluginDataFormat::TextDivider => Ok(to_text_divider(actions, divider.unwrap_or("---"))),
    }
}

fn to_yaml(actions: &[Action]) -> String {
    let mut lines = Vec::new();
    for action in actions {
        lines.push("-".to_string());
        lines.push(format!("  text: {}", yaml_scalar(&action.text)));
        lines.push(format!("  line_index: {}", action.line_index));
        lines.push(format!(
            "  project: {}",
            yaml_scalar(&action.project.clone().unwrap_or_default())
        ));
        lines.push("  project_chain:".to_string());
        for project in &action.project_chain {
            lines.push(format!("    - {}", yaml_scalar(project)));
        }
        lines.push("  notes:".to_string());
        for note in &action.notes {
            lines.push(format!("    - {}", yaml_scalar(note)));
        }
        lines.push("  tags:".to_string());
        for tag in &action.tags {
            lines.push(format!("    - {}", yaml_scalar(tag)));
        }
        lines.push("  tag_values:".to_string());
        for (key, value) in &action.tag_values {
            lines.push(format!("    {}: {}", yaml_scalar(key), yaml_scalar(value)));
        }
        lines.push(format!("  done: {}", action.done));
        lines.push(format!(
            "  due: {}",
            action
                .due
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default()
        ));
        lines.push(format!("  source_file: {}", yaml_scalar(&action.source_file)));
    }
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

fn to_csv(actions: &[Action]) -> String {
    let mut out = String::from(
        "text,line_index,project,project_chain,notes,tags,tag_values,done,due,source_file\n",
    );
    for action in actions {
        let project = action.project.clone().unwrap_or_default();
        let project_chain = action.project_chain.join("|");
        let notes = action.notes.join("|");
        let tags = action.tags.join("|");
        let tag_values = serde_json::to_string(&action.tag_values).unwrap_or_else(|_| "{}".to_string());
        let due = action
            .due
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        let row = vec![
            action.text.clone(),
            action.line_index.to_string(),
            project,
            project_chain,
            notes,
            tags,
            tag_values,
            action.done.to_string(),
            due,
            action.source_file.clone(),
        ];
        out.push_str(&row.into_iter().map(csv_escape).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}

fn csv_escape(value: String) -> String {
    let escaped = value.replace('"', "\"\"");
    format!("\"{escaped}\"")
}

fn to_text_divider(actions: &[Action], block_sep: &str) -> String {
    let mut blocks = Vec::new();
    for action in actions {
        let mut block = Vec::new();
        block.push(format!("text: {}", action.text));
        block.push(format!("line_index: {}", action.line_index));
        block.push(format!("project: {}", action.project.clone().unwrap_or_default()));
        block.push(format!("project_chain: {}", action.project_chain.join(" > ")));
        block.push(format!("tags: {}", action.tags.join(" ")));
        block.push(format!("notes: {}", action.notes.join(" | ")));
        block.push(format!("done: {}", action.done));
        block.push(format!("source_file: {}", action.source_file));
        blocks.push(block.join("\n"));
    }
    if blocks.is_empty() {
        String::new()
    } else {
        let sep = format!("\n{block_sep}\n");
        format!("{}\n", blocks.join(&sep))
    }
}

/// Merge plugin stdout back into a copy of `base`, matching rows by `source_file`/`file_path` and
/// `line_index` or 1-based `line` (Ruby-style).
pub fn merge_plugin_stdout_into_actions(
    base: &[Action],
    stdout: &str,
    format: PluginDataFormat,
    divider: Option<&str>,
) -> Result<Vec<Action>> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(base.to_vec());
    }
    match format {
        PluginDataFormat::Json => merge_after_json(base, trimmed),
        PluginDataFormat::Yaml => merge_after_json(base, trimmed).or_else(|_| {
            merge_after_text_divider(base, trimmed, divider.unwrap_or("---"))
        }),
        PluginDataFormat::Csv => merge_after_csv(base, trimmed),
        PluginDataFormat::TextDivider => {
            merge_after_text_divider(base, trimmed, divider.unwrap_or("---"))
        }
    }
}

fn merge_after_json(base: &[Action], trimmed: &str) -> Result<Vec<Action>> {
    if let Ok(full) = serde_json::from_str::<Vec<Action>>(trimmed) {
        let mut out = base.to_vec();
        for p in full {
            if let Some(slot) = out
                .iter_mut()
                .find(|a| paths_match(&a.source_file, &p.source_file) && a.line_index == p.line_index)
            {
                *slot = p;
            }
        }
        return Ok(out);
    }
    let arr: Vec<Value> = serde_json::from_str(trimmed).context("parse plugin JSON array")?;
    let mut out = base.to_vec();
    for v in arr {
        merge_json_patch(&mut out, &v)?;
    }
    Ok(out)
}

fn merge_json_patch(out: &mut Vec<Action>, v: &Value) -> Result<()> {
    let file = v
        .get("source_file")
        .or_else(|| v.get("file_path"))
        .and_then(|x| x.as_str())
        .context("plugin JSON row needs source_file or file_path")?;
    let line_idx = resolve_line_index(v).context("plugin JSON row needs line_index or line")?;
    let pos = out.iter_mut().find(|a| {
        paths_match(&a.source_file, file) && a.line_index == line_idx
    });
    let Some(action) = pos else {
        return Ok(());
    };
    apply_json_value_to_action(action, v)?;
    Ok(())
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

fn apply_json_value_to_action(action: &mut Action, v: &Value) -> Result<()> {
    let tags_arr = v.get("tags").and_then(|x| x.as_array());
    let tag_list: &[Value] = tags_arr.map(|a| a.as_slice()).unwrap_or(&[]);
    let has_ruby_style_tags = tags_arr.is_some_and(|arr| {
        arr.iter()
            .any(|t| t.as_object().and_then(|o| o.get("name")).is_some())
    });

    if let Some(t) = v.get("text").and_then(|x| x.as_str()) {
        action.text = if has_ruby_style_tags {
            merge_text_with_plugin_tag_objects(t, tag_list)?
        } else {
            t.to_string()
        };
        refresh_action_tags(action);
    } else if has_ruby_style_tags {
        let stripped = strip_inline_taskpaper_tags(&action.text);
        let extra = format_plugin_tag_objects(tag_list)?;
        action.text = if extra.is_empty() {
            stripped
        } else {
            format!("{} {}", stripped.trim_end(), extra)
        };
        refresh_action_tags(action);
    }
    if let Some(n) = v.get("notes").and_then(|x| x.as_array()) {
        action.notes = n
            .iter()
            .filter_map(|x| x.as_str().map(|s| s.to_string()))
            .collect();
    } else if let Some(n) = v.get("note").and_then(|x| x.as_str()) {
        action.notes = if n.is_empty() {
            Vec::new()
        } else {
            n.lines().map(|s| s.to_string()).collect()
        };
    }
    if let Some(ch) = v.get("project_chain").and_then(|x| x.as_array()) {
        action.project_chain = ch
            .iter()
            .filter_map(|x| x.as_str().map(|s| s.to_string()))
            .collect();
        action.project = action.project_chain.last().cloned();
    } else if let Some(ch) = v.get("parents").and_then(|x| x.as_array()) {
        let names: Vec<String> = ch
            .iter()
            .filter_map(|x| x.as_str().map(|s| s.to_string()))
            .collect();
        if !names.is_empty() {
            action.project_chain = names.clone();
            action.project = names.last().cloned();
        }
    } else if let Some(p) = v.get("project").and_then(|x| x.as_str()) {
        action.project = if p.is_empty() {
            None
        } else {
            Some(p.to_string())
        };
    }
    if let Some(d) = v.get("done").and_then(|x| x.as_bool()) {
        action.done = d;
    }
    Ok(())
}

/// Apply `text` and optional `tags` column (JSON array of `{name, value}`) like Ruby `na_gem`.
fn apply_text_tags_from_strings(
    action: &mut Action,
    text: Option<&str>,
    tags_raw: Option<&str>,
) -> Result<()> {
    let tags_trim = tags_raw.map(str::trim).filter(|s| !s.is_empty());
    let tags_json = tags_trim.and_then(|s| {
        if s.starts_with('[') {
            serde_json::from_str::<Vec<Value>>(s).ok()
        } else {
            None
        }
    });
    let has_ruby = tags_json.as_ref().is_some_and(|arr| {
        arr.iter()
            .any(|t| t.as_object().and_then(|o| o.get("name")).is_some())
    });
    if has_ruby {
        if let Some(arr) = tags_json.as_ref() {
            let base = text.unwrap_or_else(|| action.text.as_str());
            action.text = merge_text_with_plugin_tag_objects(base, arr)?;
            refresh_action_tags(action);
        }
        return Ok(());
    }
    if let Some(t) = text {
        action.text = t.to_string();
        refresh_action_tags(action);
    }
    Ok(())
}

/// Match Ruby: strip `@tag` / `@tag(...)` from text, then append tags from `{name, value}` objects.
fn merge_text_with_plugin_tag_objects(
    text: &str,
    tag_objs: &[Value],
) -> Result<String> {
    let stripped = strip_inline_taskpaper_tags(text);
    let extra = format_plugin_tag_objects(tag_objs)?;
    Ok(if extra.is_empty() {
        stripped
    } else {
        format!("{} {}", stripped.trim_end(), extra)
    })
}

/// Strip TaskPaper-style tags (Ruby `/(?<=\A| )@\S+(?:\(.*?\))?/`).
fn strip_inline_taskpaper_tags(text: &str) -> String {
    let spaced = Regex::new(r" +@\S+(?:\([^)]*\))?").expect("valid regex");
    let mut s = spaced.replace_all(text.trim(), " ").to_string();
    let leading = Regex::new(r"^@\S+(?:\([^)]*\))?").expect("valid regex");
    s = leading.replace(&s, "").to_string();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Ruby: `["@k" or "@k(v)"]` from `[{name, value}, ...]`.
fn format_plugin_tag_objects(tags: &[Value]) -> Result<String> {
    let mut parts = Vec::new();
    for t in tags {
        if let Some(obj) = t.as_object() {
            let name = obj
                .get("name")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim();
            if name.is_empty() {
                continue;
            }
            let val = obj
                .get("value")
                .and_then(|x| {
                    x.as_str()
                        .map(|s| s.to_string())
                        .or_else(|| x.as_i64().map(|n| n.to_string()))
                        .or_else(|| x.as_f64().map(|n| n.to_string()))
                })
                .unwrap_or_default();
            if val.is_empty() {
                parts.push(format!("@{name}"));
            } else {
                parts.push(format!("@{name}({val})"));
            }
        } else if let Some(s) = t.as_str() {
            if !s.is_empty() {
                parts.push(s.to_string());
            }
        }
    }
    Ok(parts.join(" "))
}

fn merge_after_csv(base: &[Action], trimmed: &str) -> Result<Vec<Action>> {
    let mut lines = trimmed.lines().filter(|l| !l.trim().is_empty());
    let header = lines.next().context("CSV output empty")?;
    let cols = parse_csv_line(header);
    let cols: Vec<&str> = cols.iter().map(|s| s.as_str()).collect();
    let idx = build_csv_column_index(&cols)?;
    let mut out = base.to_vec();
    for row_line in lines {
        let fields = parse_csv_line(row_line);
        let map = csv_row_to_map(&idx, &fields);
        let file = map.get("source_file").cloned().context("csv row source_file")?;
        let line_idx = if map.contains_key("line_index") {
            map.get("line_index")
                .context("line_index")?
                .parse::<usize>()
                .context("line_index int")?
        } else if map.contains_key("line") {
            let n: usize = map.get("line").context("line")?.parse().context("line int")?;
            if n == 0 {
                continue;
            }
            n - 1
        } else {
            continue;
        };
        let Some(action) = out.iter_mut().find(|a| {
            paths_match(&a.source_file, &file) && a.line_index == line_idx
        }) else {
            continue;
        };
        let tags_col = map.get("tags").map(|s| s.as_str());
        apply_text_tags_from_strings(action, map.get("text").map(|s| s.as_str()), tags_col)?;
        if let Some(n) = map.get("notes") {
            action.notes = if n.is_empty() {
                Vec::new()
            } else {
                n.split('|').map(|s| s.to_string()).collect()
            };
        }
        if let Some(pc) = map.get("project_chain") {
            action.project_chain = if pc.is_empty() {
                Vec::new()
            } else {
                pc.split('|').map(|s| s.to_string()).collect()
            };
            action.project = action.project_chain.last().cloned();
        } else if let Some(p) = map.get("project") {
            action.project = if p.is_empty() {
                None
            } else {
                Some(p.clone())
            };
        }
        if let Some(d) = map.get("done") {
            action.done = d == "true";
        }
    }
    Ok(out)
}

fn build_csv_column_index(header: &[&str]) -> Result<HashMap<String, usize>> {
    let mut m = HashMap::new();
    for (i, h) in header.iter().enumerate() {
        m.insert((*h).to_string(), i);
    }
    if !m.contains_key("source_file") {
        anyhow::bail!("CSV header missing source_file");
    }
    Ok(m)
}

fn csv_row_to_map(idx: &HashMap<String, usize>, fields: &[String]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (k, &i) in idx {
        if let Some(v) = fields.get(i) {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

/// Split a CSV line with quoted fields (minimal RFC-style).
fn parse_csv_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cur.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                cur.push(ch);
            }
        } else if ch == '"' {
            in_quotes = true;
        } else if ch == ',' {
            out.push(cur);
            cur = String::new();
        } else {
            cur.push(ch);
        }
    }
    out.push(cur);
    out
}

fn merge_after_text_divider(base: &[Action], trimmed: &str, sep: &str) -> Result<Vec<Action>> {
    let sep_pat = format!("\n{sep}\n");
    let blocks: Vec<&str> = trimmed.split(&sep_pat).map(str::trim).filter(|s| !s.is_empty()).collect();
    let mut out = base.to_vec();
    for block in blocks {
        let kv = parse_kv_block(block);
        let Some(file) = kv
            .get("source_file")
            .cloned()
            .or_else(|| kv.get("file_path").cloned())
        else {
            continue;
        };
        let line_idx = if let Some(s) = kv.get("line_index") {
            s.parse::<usize>().context("line_index")?
        } else if let Some(s) = kv.get("line") {
            let n = s.parse::<usize>().unwrap_or(0);
            if n == 0 {
                continue;
            }
            n - 1
        } else {
            continue;
        };
        let Some(action) = out
            .iter_mut()
            .find(|a| paths_match(&a.source_file, &file) && a.line_index == line_idx)
        else {
            continue;
        };
        let tags_src = kv
            .get("tags_json")
            .map(|s| s.as_str())
            .or_else(|| {
                kv.get("tags")
                    .filter(|s| s.trim_start().starts_with('['))
                    .map(|s| s.as_str())
            });
        apply_text_tags_from_strings(
            action,
            kv.get("text").map(|s| s.as_str()),
            tags_src,
        )?;
        if let Some(n) = kv.get("notes") {
            action.notes = if n.is_empty() {
                Vec::new()
            } else {
                n.split(" | ").map(|s| s.to_string()).collect()
            };
        }
        if let Some(pc) = kv.get("project_chain") {
            action.project_chain = if pc.is_empty() {
                Vec::new()
            } else {
                pc.split(" > ").map(|s| s.to_string()).collect()
            };
            action.project = action.project_chain.last().cloned();
        } else if let Some(p) = kv.get("project") {
            action.project = if p.is_empty() {
                None
            } else {
                Some(p.clone())
            };
        }
        if let Some(d) = kv.get("done") {
            action.done = d == "true";
        }
    }
    Ok(out)
}

fn parse_kv_block(block: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for line in block.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((k, v)) = parse_kv_line(line) {
            m.insert(k, v);
        }
    }
    m
}

/// First `key: value` per line; `tags_json:` takes the rest of the line (JSON may contain `:`).
fn parse_kv_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    const TAGS_JSON: &str = "tags_json:";
    if line.len() >= TAGS_JSON.len()
        && line[..TAGS_JSON.len()].eq_ignore_ascii_case(TAGS_JSON)
    {
        let rest = line[TAGS_JSON.len()..].trim_start();
        return Some(("tags_json".to_string(), rest.to_string()));
    }
    line.split_once(':')
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
}

fn paths_match(a: &str, b: &str) -> bool {
    normalize_path(a) == normalize_path(b)
}

fn normalize_path(p: &str) -> String {
    p.replace('\\', "/")
}

fn yaml_scalar(input: &str) -> String {
    if input.is_empty() {
        "\"\"".to_string()
    } else {
        let escaped = input.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    }
}

#[cfg(test)]
mod tests {
    use super::{merge_plugin_stdout_into_actions, serialize_actions, PluginDataFormat};
    use crate::models::action::Action;
    use std::collections::HashMap;

    fn sample_action() -> Action {
        Action {
            text: "Deep work".to_string(),
            line_index: 3,
            project: Some("Work".to_string()),
            project_chain: vec!["Work".to_string(), "Client".to_string()],
            notes: vec!["first note".to_string()],
            tags: vec!["@na".to_string(), "@home".to_string()],
            tag_values: HashMap::new(),
            done: false,
            due: None,
            source_file: "tasks.taskpaper".to_string(),
        }
    }

    #[test]
    fn serializes_actions_as_yaml() {
        let out = serialize_actions(&[sample_action()], PluginDataFormat::Yaml).expect("yaml should serialize");
        assert!(out.contains("text: \"Deep work\""));
        assert!(out.contains("project: \"Work\""));
    }

    #[test]
    fn serializes_actions_as_csv() {
        let out = serialize_actions(&[sample_action()], PluginDataFormat::Csv).expect("csv should serialize");
        assert!(out.contains("\"Deep work\""));
        assert!(out.contains("\"Work|Client\""));
    }

    #[test]
    fn serializes_actions_as_text_divider() {
        let out =
            serialize_actions(&[sample_action()], PluginDataFormat::TextDivider).expect("text should serialize");
        assert!(out.contains("text: Deep work"));
        assert!(out.contains("project_chain: Work > Client"));
    }

    #[test]
    fn merge_json_patch_updates_text_and_tags() {
        let base = vec![sample_action()];
        let stdout = r#"[{"source_file":"tasks.taskpaper","line_index":3,"text":"Updated @na @home"}]"#;
        let merged = merge_plugin_stdout_into_actions(&base, stdout, PluginDataFormat::Json, None)
            .expect("merge");
        assert_eq!(merged[0].text, "Updated @na @home");
        assert!(merged[0].tags.iter().any(|t| t == "@home"));
    }

    #[test]
    fn merge_json_ruby_tag_objects_strip_text_and_append() {
        let base = vec![sample_action()];
        let stdout = r#"[{"source_file":"tasks.taskpaper","line_index":3,"text":"Plain body @na @old","tags":[{"name":"na","value":""},{"name":"priority","value":"9"}]}]"#;
        let merged = merge_plugin_stdout_into_actions(&base, stdout, PluginDataFormat::Json, None)
            .expect("merge");
        assert!(
            merged[0].text.contains("Plain body"),
            "body: {}",
            merged[0].text
        );
        assert!(
            merged[0].text.contains("@priority(9)"),
            "tags: {}",
            merged[0].text
        );
        assert!(
            !merged[0].text.contains("@old"),
            "stripped old tag: {}",
            merged[0].text
        );
    }

    #[test]
    fn merge_csv_ruby_tag_objects_strip_text_and_append() {
        let base = vec![sample_action()];
        let stdout = concat!(
            "text,line_index,project,project_chain,notes,tags,tag_values,done,due,source_file\n",
            r#""Plain body @na @old",3,Work,"Work|Client","first note|","[{""name"":""priority"",""value"":""9""}]","{}",false,,tasks.taskpaper"#,
            "\n",
        );
        let merged = merge_plugin_stdout_into_actions(&base, stdout, PluginDataFormat::Csv, None)
            .expect("merge");
        assert!(merged[0].text.contains("Plain body"), "{}", merged[0].text);
        assert!(merged[0].text.contains("@priority(9)"), "{}", merged[0].text);
        assert!(!merged[0].text.contains("@old"), "{}", merged[0].text);
    }

    #[test]
    fn merge_text_divider_tags_json_strip_text_and_append() {
        let base = vec![sample_action()];
        let stdout = concat!(
            "text: Plain body @na @old\n",
            "line_index: 3\n",
            "source_file: tasks.taskpaper\n",
            r#"tags_json: [{"name":"priority","value":"9"}]"#,
            "\n",
        );
        let merged = merge_plugin_stdout_into_actions(
            &base,
            stdout,
            PluginDataFormat::TextDivider,
            None,
        )
        .expect("merge");
        assert!(merged[0].text.contains("Plain body"), "{}", merged[0].text);
        assert!(merged[0].text.contains("@priority(9)"), "{}", merged[0].text);
        assert!(!merged[0].text.contains("@old"), "{}", merged[0].text);
    }

    #[test]
    fn merge_text_divider_tags_key_json_array() {
        let base = vec![sample_action()];
        let stdout = concat!(
            "text: Plain body @na @old\n",
            "line_index: 3\n",
            "source_file: tasks.taskpaper\n",
            r#"tags: [{"name":"priority","value":"9"}]"#,
            "\n",
        );
        let merged = merge_plugin_stdout_into_actions(
            &base,
            stdout,
            PluginDataFormat::TextDivider,
            None,
        )
        .expect("merge");
        assert!(merged[0].text.contains("@priority(9)"), "{}", merged[0].text);
    }
}
