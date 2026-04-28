use crate::models::action::Action;

#[derive(Debug, Clone, Copy)]
pub struct OutputStyle {
    pub color: bool,
    pub na_tag: &'static str,
    pub include_notes: bool,
}

pub fn format_action(action: &Action, style: OutputStyle, filename_prefix: Option<&str>) -> String {
    let mut text = action.text.clone();
    let na_token = format!(" @{}", style.na_tag.trim_start_matches('@'));
    if text.contains(&na_token) {
        text = text.replacen(&na_token, "", 1);
    }
    if style.color {
        for tag in &action.tags {
            text = text.replace(tag, &paint(tag, AnsiColor::Yellow, true));
        }
        text = paint(&text, AnsiColor::Green, false);
    }

    let line_segment = if style.color {
        paint(&format!(":{}", action.line_index), AnsiColor::Line, false)
    } else {
        format!(":{}", action.line_index)
    };
    let parents_raw = if action.project_chain.is_empty() {
        String::new()
    } else {
        format!("[{}]", action.project_chain.join(">"))
    };
    let parents_segment = if parents_raw.is_empty() {
        String::new()
    } else if style.color {
        paint(&parents_raw, AnsiColor::Cyan, true)
    } else {
        parents_raw
    };
    let filename_segment = if let Some(filename) = filename_prefix {
        if style.color {
            paint(filename, AnsiColor::Filename, false)
        } else {
            filename.to_string()
        }
    } else {
        String::new()
    };

    let mut output = String::new();
    if !filename_segment.is_empty() {
        output.push_str(&filename_segment);
        output.push(' ');
    }
    if !parents_segment.is_empty() {
        output.push_str(&parents_segment);
        output.push(' ');
    }
    output.push_str(&line_segment);
    output.push(' ');
    output.push(' ');
    output.push_str(&text);
    if style.include_notes && !action.notes.is_empty() {
        for note in &action.notes {
            output.push('\n');
            output.push_str("    ");
            output.push_str(note);
        }
    }
    output
}

#[derive(Debug, Clone, Copy)]
enum AnsiColor {
    Green,
    Yellow,
    Cyan,
    Filename,
    Line,
}

fn paint(text: &str, color: AnsiColor, bold: bool) -> String {
    let code = match (color, bold) {
        (AnsiColor::Green, false) => "32",
        (AnsiColor::Green, true) => "1;32",
        (AnsiColor::Yellow, false) => "33",
        (AnsiColor::Yellow, true) => "1;33",
        (AnsiColor::Cyan, false) => "36",
        (AnsiColor::Cyan, true) => "1;36",
        (AnsiColor::Filename, false) => "38;2;236;204;135",
        (AnsiColor::Filename, true) => "1;38;2;236;204;135",
        (AnsiColor::Line, false) => "2;37",
        (AnsiColor::Line, true) => "1;37",
    };
    format!("\x1b[{code}m{text}\x1b[0m")
}

#[cfg(test)]
mod tests {
    use super::{format_action, OutputStyle};
    use crate::models::action::Action;
    use std::collections::HashMap;

    fn sample_action() -> Action {
        Action {
            text: "Draft report @na @priority(5)".to_string(),
            line_index: 4,
            project: Some("ProjectA".to_string()),
            project_chain: vec!["Work".to_string(), "ProjectA".to_string()],
            notes: Vec::new(),
            tags: vec!["@na".to_string(), "@priority".to_string()],
            tag_values: HashMap::new(),
            done: false,
            due: None,
            source_file: "todo.taskpaper".to_string(),
        }
    }

    #[test]
    fn formats_template_like_output_without_color() {
        let out = format_action(
            &sample_action(),
            OutputStyle {
                color: false,
                na_tag: "na",
                include_notes: false,
            },
            Some("work/task.taskpaper"),
        );
        assert_eq!(
            out,
            "work/task.taskpaper [Work>ProjectA] :4  Draft report @priority(5)"
        );
    }

    #[test]
    fn formats_without_filename_prefix() {
        let out = format_action(
            &sample_action(),
            OutputStyle {
                color: false,
                na_tag: "na",
                include_notes: false,
            },
            None,
        );
        assert_eq!(out, "[Work>ProjectA] :4  Draft report @priority(5)");
    }
}
