use crate::models::action::Action;
use anyhow::Result;

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

pub fn serialize_actions(actions: &[Action], format: PluginDataFormat) -> Result<String> {
    match format {
        PluginDataFormat::Json => Ok(serde_json::to_string(actions)?),
        PluginDataFormat::Yaml => Ok(to_yaml(actions)),
        PluginDataFormat::Csv => Ok(to_csv(actions)),
        PluginDataFormat::TextDivider => Ok(to_text_divider(actions)),
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

fn to_text_divider(actions: &[Action]) -> String {
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
        format!("{}\n", blocks.join("\n---\n"))
    }
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
    use super::{serialize_actions, PluginDataFormat};
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
}
