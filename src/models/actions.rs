use crate::models::action::Action;
use std::collections::HashSet;

pub fn first_available_per_project(
    actions: Vec<Action>,
    require_na: bool,
    na_tag: &str,
) -> Vec<Action> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let expected_tag = format!("@{}", na_tag.trim_start_matches('@'));

    for action in actions {
        if require_na
            && !action
                .tags
                .iter()
                .any(|t| t.eq_ignore_ascii_case(&expected_tag))
        {
            continue;
        }

        let key = action.project.clone().unwrap_or_default();
        if key.trim().is_empty() || seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        out.push(action);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::first_available_per_project;
    use crate::models::action::Action;
    use std::collections::HashMap;

    fn action(project: Option<&str>, text: &str, tags: &[&str]) -> Action {
        Action {
            text: text.to_string(),
            line_index: 0,
            project: project.map(ToString::to_string),
            project_chain: project.map(|p| vec![p.to_string()]).unwrap_or_default(),
            notes: Vec::new(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            tag_values: HashMap::new(),
            done: false,
            due: None,
            source_file: "sample.taskpaper".to_string(),
        }
    }

    #[test]
    fn first_available_requires_na_when_enabled() {
        let actions = vec![
            action(Some("ProjectA"), "First A", &["@na"]),
            action(Some("ProjectA"), "Second A", &["@na"]),
            action(Some("ProjectB"), "Only B", &["@na"]),
        ];
        let out = first_available_per_project(actions, true, "na");
        let projects: Vec<String> = out.iter().filter_map(|a| a.project.clone()).collect();
        assert_eq!(
            projects,
            vec!["ProjectA".to_string(), "ProjectB".to_string()]
        );
        assert!(out[0].text.contains("First A"));
    }

    #[test]
    fn first_available_without_na_skips_done_and_empty_project_upstream() {
        let actions = vec![
            action(None, "Inbox action", &["@na"]),
            action(Some("ProjectA"), "First A no tag", &[]),
            action(Some("ProjectA"), "Second A @na", &["@na"]),
            action(Some("ProjectB"), "First B no tag", &[]),
        ];
        let out = first_available_per_project(actions, false, "na");
        let projects: Vec<String> = out.iter().filter_map(|a| a.project.clone()).collect();
        assert_eq!(
            projects,
            vec!["ProjectA".to_string(), "ProjectB".to_string()]
        );
        assert_eq!(out[0].text, "First A no tag");
        assert_eq!(out[1].text, "First B no tag");
    }
}
