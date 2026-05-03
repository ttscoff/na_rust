use crate::models::action::Action;
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;

fn indentation_width(line: &str) -> usize {
    line.chars().take_while(|ch| ch.is_whitespace()).count()
}

pub fn extract_actions(lines: &[String], path: &Path) -> Vec<Action> {
    let tag_re = Regex::new(r"@([a-zA-Z0-9_\\-]+)(?:\((.*?)\))?").expect("valid regex");
    let mut project_stack: Vec<(usize, String)> = Vec::new();
    let mut out = Vec::new();
    let mut idx = 0usize;

    while idx < lines.len() {
        let line = lines[idx].trim_end();
        let trimmed = line.trim();
        let indent = indentation_width(line);

        if trimmed.ends_with(':') && !trimmed.starts_with("- ") {
            let project_name = trimmed.trim_end_matches(':').trim().to_string();
            while project_stack
                .last()
                .is_some_and(|(project_indent, _)| *project_indent >= indent)
            {
                project_stack.pop();
            }
            project_stack.push((indent, project_name));
            idx += 1;
            continue;
        }

        if !trimmed.starts_with("- ") {
            idx += 1;
            continue;
        }

        let text = trimmed.trim_start_matches("- ").to_string();
        let mut tags = Vec::new();
        let mut tag_values: HashMap<String, String> = HashMap::new();
        for cap in tag_re.captures_iter(trimmed) {
            let key = cap[1].to_string();
            tags.push(format!("@{key}"));
            if let Some(value) = cap.get(2) {
                tag_values.insert(key.to_ascii_lowercase(), value.as_str().trim().to_string());
            }
        }
        let done = tags.iter().any(|t| t == "@done");
        let project_chain: Vec<String> =
            project_stack.iter().map(|(_, name)| name.clone()).collect();
        let mut notes = Vec::new();
        let mut note_idx = idx + 1;
        while note_idx < lines.len() {
            let candidate_line = lines[note_idx].trim_end();
            let candidate_trimmed = candidate_line.trim();
            let candidate_indent = indentation_width(candidate_line);

            if candidate_trimmed.is_empty() {
                notes.push(String::new());
                note_idx += 1;
                continue;
            }

            if candidate_trimmed.ends_with(':') && !candidate_trimmed.starts_with("- ") {
                break;
            }
            if candidate_trimmed.starts_with("- ") {
                break;
            }
            if candidate_indent <= indent {
                break;
            }

            notes.push(candidate_trimmed.to_string());
            note_idx += 1;
        }

        out.push(Action {
            text,
            line_index: idx,
            project: project_chain.last().cloned(),
            project_chain,
            notes,
            tags,
            tag_values,
            done,
            due: None,
            source_file: path.display().to_string(),
        });
        idx = note_idx;
    }

    out
}

/// Recompute `tags`, `tag_values`, and `done` from [`Action::text`] (TaskPaper action line body).
pub fn refresh_action_tags(action: &mut Action) {
    let tag_re = Regex::new(r"@([a-zA-Z0-9_\\-]+)(?:\((.*?)\))?").expect("valid regex");
    let trimmed = format!("- {}", action.text);
    let trimmed = trimmed.trim();
    let mut tags = Vec::new();
    let mut tag_values = HashMap::new();
    for cap in tag_re.captures_iter(trimmed) {
        let key = cap[1].to_string();
        tags.push(format!("@{key}"));
        if let Some(value) = cap.get(2) {
            tag_values.insert(key.to_ascii_lowercase(), value.as_str().trim().to_string());
        }
    }
    action.tags = tags;
    action.tag_values = tag_values;
    action.done = action.tags.iter().any(|t| t == "@done");
}

pub fn render_lines(lines: &[String]) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_actions, refresh_action_tags, render_lines};
    use crate::models::action::Action;
    use std::path::Path;

    #[test]
    fn parses_project_and_action_lines() {
        let lines = vec![
            "House:".to_string(),
            "  - Take out trash @home".to_string(),
            "  - Mow lawn @weekend".to_string(),
        ];

        let actions = extract_actions(&lines, Path::new("sample.taskpaper"));
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].project.as_deref(), Some("House"));
        assert_eq!(actions[0].project_chain, vec!["House".to_string()]);
        assert!(actions[0].tags.contains(&"@home".to_string()));
    }

    #[test]
    fn parses_tag_values() {
        let lines = vec!["- Task @priority(5) @due(2026-04-15)".to_string()];
        let actions = extract_actions(&lines, Path::new("sample.taskpaper"));
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].tag_value("@priority"), Some("5"));
        assert_eq!(actions[0].tag_value("due"), Some("2026-04-15"));
    }

    #[test]
    fn parses_nested_projects_with_parent_chain() {
        let lines = vec![
            "Work:".to_string(),
            "  ClientA:".to_string(),
            "    - Draft proposal @na".to_string(),
            "  ClientB:".to_string(),
            "    - Send update @na".to_string(),
        ];
        let actions = extract_actions(&lines, Path::new("nested.taskpaper"));
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].project.as_deref(), Some("ClientA"));
        assert_eq!(
            actions[0].project_chain,
            vec!["Work".to_string(), "ClientA".to_string()]
        );
        assert_eq!(actions[1].project.as_deref(), Some("ClientB"));
        assert_eq!(
            actions[1].project_chain,
            vec!["Work".to_string(), "ClientB".to_string()]
        );
    }

    #[test]
    fn parses_action_note_blocks() {
        let lines = vec![
            "Work:".to_string(),
            "  - Draft proposal @na".to_string(),
            "    outline opening section".to_string(),
            "    include timeline".to_string(),
            "  - Ship draft @na".to_string(),
        ];
        let actions = extract_actions(&lines, Path::new("notes.taskpaper"));
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions[0].notes,
            vec![
                "outline opening section".to_string(),
                "include timeline".to_string()
            ]
        );
        assert!(actions[1].notes.is_empty());
    }

    #[test]
    fn refresh_action_tags_updates_from_text() {
        let mut a = Action {
            text: "Task @priority(3)".to_string(),
            line_index: 0,
            project: None,
            project_chain: vec![],
            notes: vec![],
            tags: vec![],
            tag_values: std::collections::HashMap::new(),
            done: false,
            due: None,
            source_file: "x.taskpaper".to_string(),
        };
        refresh_action_tags(&mut a);
        assert_eq!(a.tag_value("priority"), Some("3"));
    }

    #[test]
    fn render_lines_round_trips_note_blocks() {
        let lines = vec![
            "Work:".to_string(),
            "  - Draft proposal @na".to_string(),
            "    outline opening section".to_string(),
            "    include timeline".to_string(),
        ];
        let rendered = render_lines(&lines);
        assert_eq!(
            rendered,
            "Work:\n  - Draft proposal @na\n    outline opening section\n    include timeline\n"
        );
    }
}
