use crate::models::action::Action;
use crate::output::color_template::expand_color_template;
use crate::output::theme::Theme;
use regex::Regex;
use std::sync::OnceLock;

/// TaskPaper `@tag` / `@tag(value)` segments for flat / nested bodies (Ruby `String#highlight_tags`).
fn tag_token_regex() -> &'static Regex {
    static RX: OnceLock<Regex> = OnceLock::new();
    RX.get_or_init(|| {
        Regex::new(
            r"(?P<pre>(?-u:^|[[:space:]]))(?P<tag>@[A-Za-z0-9_-]+)(?:(?P<lparen>\()(?P<val>[^)]*)(?P<rparen>\)))",
        )
        .expect("tag highlight regex")
    })
}

#[derive(Debug, Clone, Copy)]
pub struct OutputStyle {
    pub color: bool,
    pub na_tag: &'static str,
    pub include_notes: bool,
    /// When set, nested modes wrap action bodies to this terminal width (`$COLUMNS`). Flat lists
    /// only use wrap when **`color`** is false (single long line under full color themes).
    pub wrap_width: Option<usize>,
}

impl Default for OutputStyle {
    fn default() -> Self {
        Self {
            color: false,
            na_tag: "na",
            include_notes: false,
            wrap_width: None,
        }
    }
}

/// Strip the next-action token once, matching Ruby `Action#pretty` (`sub(/ @#{NA.na_tag}\b/, '')`).
///
/// Nested `--nest` / `--omnifocus` lists use the full action line in `na_gem` (`Actions.output`); do not use this there.
pub fn strip_na_token(text: String, na_tag: &str) -> String {
    let key = na_tag.trim_start_matches('@');
    let Ok(re) = Regex::new(&format!(r"(?iu) @{}\b", regex::escape(key))) else {
        return text;
    };
    re.replacen(&text, 1, "").into_owned()
}

/// Remove ANSI CSI / OSC escapes so string length reflects visible terminal columns.
pub(crate) fn strip_ansi_measurement(input: &str) -> String {
    static CSI: OnceLock<Regex> = OnceLock::new();
    let csi = CSI.get_or_init(|| {
        Regex::new(r"\x1b\[[\x30-\x3f]*[\x20-\x2f]*[\x40-\x7e]")
            .expect("csi strip regex")
    });
    let s = csi.replace_all(input, "").to_string();
    static OSC: OnceLock<Regex> = OnceLock::new();
    let osc = OSC.get_or_init(|| Regex::new(r"\x1b\][^\x07]*\x07").expect("osc strip regex"));
    osc.replace_all(&s, "").to_string()
}

/// Visible width assuming 8-column tab stops (`\t`), after stripping ANSI escapes.
///
/// Used for `--nest` / `--omnifocus` wrapping so continuation lines align under the first prefix.
pub(crate) fn visual_width_tabs8(s: &str) -> usize {
    let stripped = strip_ansi_measurement(s);
    let mut col = 0usize;
    for ch in stripped.chars() {
        if ch == '\t' {
            col = ((col + 8) / 8) * 8;
        } else if ch != '\n' && ch != '\r' {
            col += 1;
        }
    }
    col
}

pub(crate) fn wrap_words(input: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![input.to_string()];
    }
    let mut lines = Vec::new();
    let mut cur = String::new();
    for w in input.split_whitespace() {
        let wlen = w.chars().count();
        if cur.is_empty() {
            cur.push_str(w);
            continue;
        }
        let clen = cur.chars().count();
        if clen + 1 + wlen <= width {
            cur.push(' ');
            cur.push_str(w);
        } else {
            lines.push(std::mem::take(&mut cur));
            cur.push_str(w);
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

pub(crate) fn paint_themed(text: &str, template: &str) -> String {
    format!(
        "{}{}\x1b[0m",
        expand_color_template(template),
        text
    )
}

/// Flat-line layout: Rust-style (`%filename%`, …) and Ruby-style (`%filename`, `%line`, …) tokens.
/// `%parents` / `%parent` / `%parents%`: bracketed hierarchy segment plus trailing space when non-empty.
/// `%project` / `%project%`: leaf project name only (no brackets).
///
/// Ruby themes often abut tokens (`%filename%line%parents`) where the `%` between `filename` and
/// `line` is shared between `%filename` and `%line`, so naive `%filename%` replacement breaks.
/// We scan left-to-right and match **longest / most specific prefixes first** (including Rust `%%`
/// forms such as `%filename%%line%`).
fn substitute_output_template(
    template: &str,
    filename: &str,
    line: &str,
    parents_prefix: &str,
    project: &str,
    action: &str,
    note: &str,
) -> String {
    let mut out = String::with_capacity(template.len() + filename.len() + line.len());
    let mut rest = template;
    while !rest.is_empty() {
        // Rust doubled `%` between tokens
        if let Some(r) = rest.strip_prefix("%filename%%line%") {
            out.push_str(filename);
            out.push_str(line);
            rest = r;
            continue;
        }
        // Ruby default theme adjacency: `%filename` + `%line` → literal `%filename%line`
        if let Some(r) = rest.strip_prefix("%filename%line") {
            out.push_str(filename);
            out.push_str(line);
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix("%filename%") {
            out.push_str(filename);
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix("%filename") {
            out.push_str(filename);
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix("%line%") {
            out.push_str(line);
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix("%line") {
            out.push_str(line);
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix("%parents%") {
            out.push_str(parents_prefix);
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix("%parents") {
            out.push_str(parents_prefix);
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix("%parent%") {
            out.push_str(parents_prefix);
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix("%parent") {
            out.push_str(parents_prefix);
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix("%project%") {
            out.push_str(project);
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix("%project") {
            out.push_str(project);
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix("%action%") {
            out.push_str(action);
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix("%action") {
            out.push_str(action);
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix("%note%") {
            out.push_str(note);
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix("%note") {
            out.push_str(note);
            rest = r;
            continue;
        }
        let mut it = rest.chars();
        out.push(it.next().unwrap());
        rest = it.as_str();
    }
    out
}

/// Ruby `String#highlight_tags` + leading `template[:action]` color: `@name`, `(` `)` and value use
/// separate theme slots; action color resumes after each tag.
pub(crate) fn color_action_body(body: &str, _tags: &[String], style_color: bool, theme: &Theme) -> String {
    if !style_color {
        return body.to_string();
    }
    let action_open = expand_color_template(&theme.action);
    let re = tag_token_regex();
    let mut buf = String::with_capacity(body.len() + body.len() / 4);
    buf.push_str(&action_open);
    let mut ix = 0usize;
    for cap in re.captures_iter(body) {
        let whole = cap.get(0).expect("tag match");
        buf.push_str(&body[ix..whole.start()]);
        let pre = cap.name("pre").map(|m| m.as_str()).unwrap_or("");
        let tag = cap.name("tag").map(|m| m.as_str()).unwrap_or("");
        let val = cap.name("val").map(|m| m.as_str());
        buf.push_str(&highlight_one_tag(pre, tag, val, theme, &action_open));
        ix = whole.end();
    }
    buf.push_str(&body[ix..]);
    buf.push_str("\x1b[0m");
    buf
}

fn highlight_one_tag(
    pre: &str,
    tag: &str,
    val: Option<&str>,
    theme: &Theme,
    action_reopen: &str,
) -> String {
    let t_open = expand_color_template(&theme.tags);
    let p_open = expand_color_template(&theme.value_parens);
    let v_open = expand_color_template(&theme.values);
    match val {
        Some(v) => format!(
            "{pre}{t_open}{tag}\x1b[0m{p_open}(\x1b[0m{v_open}{v}\x1b[0m{p_open})\x1b[0m{action_reopen}",
            pre = pre,
            t_open = t_open,
            tag = tag,
            p_open = p_open,
            v_open = v_open,
            v = v,
            action_reopen = action_reopen,
        ),
        None => format!(
            "{pre}{t_open}{tag}\x1b[0m{action_reopen}",
            pre = pre,
            t_open = t_open,
            tag = tag,
            action_reopen = action_reopen,
        ),
    }
}

/// `[a/b/c]` segment for `--nest` / `--omnifocus` lines (full chain, not leaf-only).
pub(crate) fn nested_bracketed_chain(chain: &str, style_color: bool, theme: &Theme) -> String {
    if !style_color {
        return format!("[{chain}]");
    }
    format!(
        "{}{}{}",
        paint_themed("[", &theme.bracket),
        paint_themed(chain, &theme.parent),
        paint_themed("]", &theme.bracket),
    )
}

/// One nested file-group header (`path:`) using the filename color template when enabled.
pub(crate) fn nested_file_title(display: &str, style_color: bool, theme: &Theme) -> String {
    if !style_color {
        format!("{display}:")
    } else {
        paint_themed(&format!("{display}:"), &theme.filename)
    }
}

/// Format one action line using `theme.flat_action_template(...)`:
/// - **`templates.output`** (when non-empty): sole layout for flat output (Ruby `templates.output`); overrides mode-specific templates.
/// - **multi-file** (`filename_prefix` **Some**): `templates.multi_file` when `output` is empty.
/// - **`--no-file`**: `templates.no_file` when set and `output` is empty.
/// - **single-file**: `templates.single_file` if set, else `templates.default`.
///
/// Placeholders: **`%filename%`** / **`%filename`**, **`%line%`** / **`%line`**, **`%parents%`** / **`%parents`** / **`%parent`**, **`%project%`** / **`%project`**, **`%action%`** / **`%action`**, **`%note%`** / **`%note`** (notes body when `--notes`; omitted from the trailing block if present in the template).
///
/// Project labels render as `[leaf]` with **`bracket`** styling on `[` / `]` and **`parent`** on the name.
///
/// When notes exist but `include_notes` is false, appends `*` (Ruby `template[:note]*`).
pub fn format_action(
    action: &Action,
    style: OutputStyle,
    filename_prefix: Option<&str>,
    theme: &Theme,
    omit_filename: bool,
) -> String {
    let plain_body = strip_na_token(action.text.clone(), style.na_tag);

    let line_body = format!(":{}", action.line_index);
    let leaf_project = action
        .project_chain
        .last()
        .cloned()
        .or_else(|| action.project.clone());
    let parents_plain = leaf_project
        .as_ref()
        .map(|p| format!("[{}]", p))
        .unwrap_or_default();

    let multi_file = filename_prefix.is_some();
    let layout_tpl = theme.flat_action_template(multi_file, omit_filename);
    let template_inlines_notes = layout_tpl.contains("%note");

    let plain_parents_prefix = if leaf_project.is_some() {
        format!("{} ", parents_plain)
    } else {
        String::new()
    };

    let plain_project = leaf_project.clone().unwrap_or_default();

    let prefix_len_plain = if !style.color && style.wrap_width.is_some() {
        substitute_output_template(
            layout_tpl,
            filename_prefix.unwrap_or(""),
            &line_body,
            &plain_parents_prefix,
            &plain_project,
            "",
            "",
        )
        .chars()
        .count()
    } else {
        0
    };

    let body_lines: Vec<String> = if !style.color && style.wrap_width.is_some() {
        let cols = style.wrap_width.unwrap();
        let avail = cols.saturating_sub(prefix_len_plain).max(12);
        wrap_words(&plain_body, avail)
    } else {
        vec![plain_body]
    };

    let parents_segment = if let Some(ref leaf) = leaf_project {
        if style.color {
            format!(
                "{}{}{}",
                paint_themed("[", &theme.bracket),
                paint_themed(leaf, &theme.parent),
                paint_themed("]", &theme.bracket),
            )
        } else {
            parents_plain.clone()
        }
    } else {
        String::new()
    };

    let line_segment = if style.color {
        paint_themed(&line_body, &theme.line)
    } else {
        line_body.clone()
    };

    let filename_segment = if let Some(filename) = filename_prefix {
        if style.color {
            paint_themed(filename, &theme.filename)
        } else {
            filename.to_string()
        }
    } else {
        String::new()
    };

    let parents_with_space = if parents_segment.is_empty() {
        String::new()
    } else {
        format!("{parents_segment} ")
    };

    let project_segment = if let Some(ref leaf) = leaf_project {
        if style.color {
            paint_themed(leaf, &theme.project)
        } else {
            leaf.clone()
        }
    } else {
        String::new()
    };

    let note_for_template = if style.include_notes && !action.notes.is_empty() {
        format!("\n    {}", action.notes.join("\n    "))
    } else {
        String::new()
    };

    let mut blocks = Vec::new();
    for (i, chunk) in body_lines.iter().enumerate() {
        let text = color_action_body(chunk, &action.tags, style.color, theme);

        let mut line_out = String::new();
        if i > 0 {
            line_out.push_str(&" ".repeat(prefix_len_plain));
            line_out.push_str(&text);
        } else {
            line_out.push_str(&substitute_output_template(
                layout_tpl,
                filename_segment.as_str(),
                line_segment.as_str(),
                parents_with_space.as_str(),
                project_segment.as_str(),
                &text,
                note_for_template.as_str(),
            ));
        }

        blocks.push(line_out);
    }

    let mut output = blocks.join("\n");

    if !style.include_notes && !action.notes.is_empty() {
        if style.color {
            output.push_str(&paint_themed("*", &theme.note));
        } else {
            output.push('*');
        }
    }

    if style.include_notes && !action.notes.is_empty() && !template_inlines_notes {
        for note in &action.notes {
            output.push('\n');
            output.push_str("    ");
            output.push_str(note);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{format_action, strip_na_token, OutputStyle};
    use crate::output::theme::Theme;
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
    fn formats_multi_file_line_before_project_like_ruby_theme() {
        let out = format_action(
            &sample_action(),
            OutputStyle {
                wrap_width: None,
                ..Default::default()
            },
            Some("work/task.taskpaper"),
            &Theme::default(),
            false,
        );
        assert_eq!(
            out,
            "work/task.taskpaper:4 [ProjectA] Draft report @priority(5)"
        );
    }

    #[test]
    fn formats_without_filename_prefix() {
        let out = format_action(
            &sample_action(),
            OutputStyle {
                wrap_width: None,
                ..Default::default()
            },
            None,
            &Theme::default(),
            false,
        );
        assert_eq!(out, "[ProjectA] :4 Draft report @priority(5)");
    }

    #[test]
    fn appends_note_marker_when_notes_hidden() {
        let mut a = sample_action();
        a.notes = vec!["Note line".to_string()];
        let out = format_action(
            &a,
            OutputStyle {
                include_notes: false,
                wrap_width: None,
                ..Default::default()
            },
            None,
            &Theme::default(),
            false,
        );
        assert!(out.ends_with('*'));
        assert!(!out.contains("Note line"));
    }

    #[test]
    fn strip_na_token_removes_tag() {
        assert_eq!(
            strip_na_token("Task @na @done".to_string(), "na"),
            "Task @done"
        );
    }

    #[test]
    fn strip_na_token_word_boundary_not_narrow() {
        assert_eq!(
            strip_na_token("Task @narrow @done".to_string(), "na"),
            "Task @narrow @done"
        );
    }

    #[test]
    fn color_action_body_highlights_tag_name_parens_and_value_like_ruby() {
        let theme = Theme::default();
        let body = "Do it @priority(5) now";
        let out = super::color_action_body(body, &[], true, &theme);
        assert!(out.starts_with(&crate::output::color_template::expand_color_template("{g}")));
        assert!(
            out.contains("\x1b[35m@priority\x1b[0m"),
            "tag uses theme tags (magenta): {out:?}"
        );
        assert!(
            out.contains("\x1b[35m(\x1b[0m"),
            "parens use value_parens: {out:?}"
        );
        assert!(
            out.contains("\x1b[36m5\x1b[0m"),
            "value uses theme values (cyan): {out:?}"
        );
    }

    #[test]
    fn custom_single_file_template_reorders_segments() {
        let mut theme = Theme::default();
        theme.templates.single_file = "%line% %parents%%action%".to_string();
        let out = format_action(
            &sample_action(),
            OutputStyle {
                wrap_width: None,
                ..Default::default()
            },
            None,
            &theme,
            false,
        );
        assert_eq!(
            out,
            ":4 [ProjectA] Draft report @priority(5)"
        );
    }

    #[test]
    fn substitute_output_template_unit() {
        assert_eq!(
            super::substitute_output_template(
                "%filename%%line% %parents%%action%",
                "a.taskpaper",
                ":1",
                "[P] ",
                "Proj",
                "Todo",
                "",
            ),
            "a.taskpaper:1 [P] Todo"
        );
    }

    /// Ruby default `templates.output` abuts `%filename` + `%line` + `%parents` (no `%%` delimiters).
    #[test]
    fn substitute_ruby_style_abutting_tokens() {
        assert_eq!(
            super::substitute_output_template(
                "%filename%line%parents| %action%",
                "a.taskpaper",
                ":1",
                "[P] ",
                "leaf",
                "body",
                "",
            ),
            "a.taskpaper:1[P] | body"
        );
    }

    #[test]
    fn templates_output_overrides_layout() {
        let mut theme = Theme::default();
        theme.templates.output = "%project% %line% %action%".to_string();
        let out = format_action(
            &sample_action(),
            OutputStyle {
                wrap_width: None,
                ..Default::default()
            },
            Some("w.taskpaper"),
            &theme,
            false,
        );
        assert_eq!(out, "ProjectA :4 Draft report @priority(5)");
    }

    #[test]
    fn omit_filename_uses_templates_no_file() {
        let mut theme = Theme::default();
        theme.templates.no_file = "[%action%]%line%".to_string();
        let out = format_action(
            &sample_action(),
            OutputStyle {
                wrap_width: None,
                ..Default::default()
            },
            None,
            &theme,
            true,
        );
        assert_eq!(out, "[Draft report @priority(5)]:4");
    }

    #[test]
    fn parents_use_bracket_and_parent_templates_separately() {
        let mut theme = Theme::default();
        theme.bracket = "{m}".to_string();
        theme.parent = "{c}".to_string();
        let out = format_action(
            &sample_action(),
            OutputStyle {
                color: true,
                wrap_width: None,
                ..Default::default()
            },
            None,
            &theme,
            false,
        );
        assert!(out.contains("\x1b[35m"), "magenta bracket template: {out:?}");
        assert!(out.contains("\x1b[36m"), "cyan parent template: {out:?}");
    }

    #[test]
    fn nested_bracket_plain_matches_chain_segment() {
        assert_eq!(
            super::nested_bracketed_chain("Work/A", false, &Theme::default()),
            "[Work/A]"
        );
    }

    #[test]
    fn nested_file_title_plain() {
        assert_eq!(
            super::nested_file_title("todo.taskpaper", false, &Theme::default()),
            "todo.taskpaper:"
        );
    }

    #[test]
    fn visual_width_tabs8_accounts_for_tabs_before_strip() {
        assert_eq!(super::visual_width_tabs8("\t"), 8);
        assert_eq!(super::visual_width_tabs8("\t-\t"), 16);
        assert_eq!(
            super::visual_width_tabs8(&format!(
                "{}x",
                crate::output::color_template::expand_color_template("{g}")
            )),
            1
        );
    }
}
