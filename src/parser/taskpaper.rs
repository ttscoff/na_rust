use crate::models::action::Action;
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;

/// TaskPaper project header: `Name:` with optional `@tags` after the colon (Ruby `String#project?`).
pub fn parse_project_header(trimmed: &str) -> Option<String> {
    if trimmed.starts_with("- ") {
        return None;
    }
    let (name_part, after_colon) = trimmed.split_once(':')?;
    let name = name_part.trim();
    if name.is_empty() {
        return None;
    }
    let suffix = after_colon.trim();
    if !suffix.is_empty()
        && !suffix
            .split_whitespace()
            .all(|token| token.starts_with('@'))
    {
        return None;
    }
    Some(name.to_string())
}

/// TaskPaper indent level: leading tabs, with runs of 4 spaces treated as one tab (Ruby `indent_level`).
pub fn taskpaper_indent_level(line: &str) -> usize {
    let prefix: String = line.chars().take_while(|ch| ch.is_whitespace()).collect();
    if prefix.is_empty() {
        return 0;
    }
    prefix
        .replace("    ", "\t")
        .chars()
        .filter(|&c| c == '\t')
        .count()
}

/// Parent chain for an action at `action_indent` (Ruby `NA::Todo` effective_parent).
fn effective_project_chain(
    project_stack: &[(usize, String)],
    action_indent: usize,
) -> Vec<String> {
    if project_stack.is_empty() {
        return Vec::new();
    }
    if let Some(chosen_index) = project_stack
        .iter()
        .enumerate()
        .rev()
        .find_map(|(i, (proj_indent, _))| (*proj_indent < action_indent).then_some(i))
    {
        project_stack[..=chosen_index]
            .iter()
            .map(|(_, name)| name.clone())
            .collect()
    } else {
        project_stack
            .iter()
            .map(|(_, name)| name.clone())
            .collect()
    }
}

pub fn extract_actions(lines: &[String], path: &Path) -> Vec<Action> {
    let tag_re = Regex::new(r"@([a-zA-Z0-9_\\-]+)(?:\((.*?)\))?").expect("valid regex");
    let mut project_stack: Vec<(usize, String)> = Vec::new();
    let mut out = Vec::new();
    let mut idx = 0usize;

    while idx < lines.len() {
        let line = lines[idx].trim_end();
        let trimmed = line.trim();
        let indent = taskpaper_indent_level(line);

        if let Some(project_name) = parse_project_header(trimmed) {
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
        let project_chain = effective_project_chain(&project_stack, indent);
        let mut notes = Vec::new();
        let mut note_idx = idx + 1;
        while note_idx < lines.len() {
            let candidate_line = lines[note_idx].trim_end();
            let candidate_trimmed = candidate_line.trim();
            let candidate_indent = taskpaper_indent_level(candidate_line);

            if candidate_trimmed.is_empty() {
                notes.push(String::new());
                note_idx += 1;
                continue;
            }

            if parse_project_header(candidate_trimmed).is_some() {
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
            "\t- Take out trash @home".to_string(),
            "\t- Mow lawn @weekend".to_string(),
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
            "\tClientA:".to_string(),
            "\t\t- Draft proposal @na".to_string(),
            "\tClientB:".to_string(),
            "\t\t- Send update @na".to_string(),
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
            "\t- Draft proposal @na".to_string(),
            "\t\toutline opening section".to_string(),
            "\t\tinclude timeline".to_string(),
            "\t- Ship draft @na".to_string(),
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
    fn parses_project_lines_with_trailing_tags() {
        let lines = vec![
            "Inbox: @bucket @.todo".to_string(),
            "\tNew Videos:".to_string(),
            "\t\t- under subproject".to_string(),
            "\t- back in inbox".to_string(),
        ];
        let actions = extract_actions(&lines, Path::new("tagged-project.taskpaper"));
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].project_chain, vec!["Inbox", "New Videos"]);
        assert_eq!(actions[1].project_chain, vec!["Inbox"]);
    }

    #[test]
    fn actions_after_subproject_outdent_use_parent_only() {
        let lines = vec![
            "Inbox:".to_string(),
            "\t- before subproject @na".to_string(),
            "\tNew Videos:".to_string(),
            "\t\t- under New Videos @na".to_string(),
            "\t\t- also under New Videos".to_string(),
            "\t- after subproject @na".to_string(),
            "\t- still inbox @na".to_string(),
        ];
        let actions = extract_actions(&lines, Path::new("inbox.taskpaper"));
        assert_eq!(actions.len(), 5);
        assert_eq!(actions[0].project_chain, vec!["Inbox"]);
        assert_eq!(actions[1].project_chain, vec!["Inbox", "New Videos"]);
        assert_eq!(actions[2].project_chain, vec!["Inbox", "New Videos"]);
        assert_eq!(actions[3].project_chain, vec!["Inbox"]);
        assert_eq!(actions[4].project_chain, vec!["Inbox"]);
    }

    #[test]
    fn indentation_level_counts_tabs_and_space_groups() {
        assert_eq!(super::taskpaper_indent_level("\t- action"), 1);
        assert_eq!(super::taskpaper_indent_level("    - action"), 1);
        assert_eq!(super::taskpaper_indent_level("\t\t- action"), 2);
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
            "\t- Draft proposal @na".to_string(),
            "\t\toutline opening section".to_string(),
            "\t\tinclude timeline".to_string(),
        ];
        let rendered = render_lines(&lines);
        assert_eq!(
            rendered,
            "Work:\n\t- Draft proposal @na\n\t\toutline opening section\n\t\tinclude timeline\n"
        );
    }
}
