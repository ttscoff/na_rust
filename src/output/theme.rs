//! Load and merge `theme.yaml` with defaults (Ruby `NA::Theme`–compatible keys).
//!
//! Flat action bodies mirror Ruby `highlight_tags`: `@tag`, parenthesis around values, and the
//! value text use **`tags`**, **`value_parens`**, and **`values`** respectively (`NA::Theme` keys).

use crate::io::xdg::na_data_dir;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;

/// Optional overlay from YAML (missing keys keep defaults).
#[derive(Debug, Default, Deserialize, Serialize)]
struct ThemeOverlay {
    parent: Option<String>,
    bracket: Option<String>,
    /// Leaf project name (`%project` in Ruby `templates.output`).
    project: Option<String>,
    action: Option<String>,
    tags: Option<String>,
    /// Parentheses wrapping tag values (`value_parens` in Ruby themes).
    value_parens: Option<String>,
    /// Value inside `@tag(...)` (`values` in Ruby themes).
    values: Option<String>,
    filename: Option<String>,
    line: Option<String>,
    note: Option<String>,
    templates: Option<TemplatesOverlay>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct TemplatesOverlay {
    #[serde(rename = "default")]
    default_tpl: Option<String>,
    single_file: Option<String>,
    multi_file: Option<String>,
    no_file: Option<String>,
    output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub parent: String,
    pub bracket: String,
    /// Styled leaf project name for `%project` / `%project%` (Ruby `theme[:project]`).
    pub project: String,
    pub action: String,
    pub tags: String,
    /// Colors `(` `)` around tag values (`NA::Theme` `value_parens`).
    pub value_parens: String,
    /// Colors the inner value of `@tag(value)` (`NA::Theme` `values`).
    pub values: String,
    pub filename: String,
    pub line: String,
    pub note: String,
    pub templates: ThemeTemplates,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeTemplates {
    /// Ruby key `default`
    pub default_tpl: String,
    pub single_file: String,
    pub multi_file: String,
    /// Used with `next`/`find` `--no-file` (`--no-files`). Ruby default matches `default_tpl`.
    pub no_file: String,
    pub output: String,
}

impl Default for ThemeTemplates {
    /// Builtin layouts aligned with Ruby `NA::Theme` defaults (`%parents %line %action`, etc.).
    fn default() -> Self {
        Self {
            default_tpl: "%parents%%line% %action%".to_string(),
            single_file: String::new(),
            // Ruby `theme[:templates][:multi_file]` abuts `%filename%`, `%line%`, and `%parents%`; the
            // line segment itself ends with a trailing space (see `format_action` / Ruby `line_num`).
            multi_file: "%filename%%line%%parents% %action%".to_string(),
            no_file: String::new(),
            output: String::new(), // When non-empty, flat layout for all modes (Ruby `templates.output`).
        }
    }
}

impl Default for Theme {
    /// Matches the previous hard-coded ANSI roles in `formatter::paint`.
    fn default() -> Self {
        Self {
            parent: "{bc}".to_string(),
            bracket: "{bc}".to_string(),
            project: "{bc}".to_string(),
            action: "{g}".to_string(),
            tags: "{m}".to_string(),
            value_parens: "{m}".to_string(),
            values: "{c}".to_string(),
            filename: "{#eccc87}".to_string(),
            line: "{dw}".to_string(),
            note: "{dw}".to_string(),
            templates: ThemeTemplates::default(),
        }
    }
}

impl Theme {
    /// Resolved layout string for flat terminal actions (`next` / `find` / `completed`).
    ///
    /// `omit_filename` mirrors Ruby `no_files` / CLI `--no-file` (omit filename column even when
    /// multiple todo files are in play).
    ///
    /// When **`templates.output`** is non-empty in `theme.yaml`, it is the layout for flat lists
    /// and overrides `default` / `single_file` / `multi_file` / `no_file` (same idea as Ruby
    /// passing the selected template into `Action.pretty` via the `output` slot).
    pub fn flat_action_template(&self, multi_file: bool, omit_filename: bool) -> &str {
        if !self.templates.output.is_empty() {
            return self.templates.output.as_str();
        }
        if multi_file {
            if self.templates.multi_file.is_empty() {
                "%filename%%line%%parents% %action%"
            } else {
                self.templates.multi_file.as_str()
            }
        } else if omit_filename && !self.templates.no_file.is_empty() {
            self.templates.no_file.as_str()
        } else if !self.templates.single_file.is_empty() {
            self.templates.single_file.as_str()
        } else if !self.templates.default_tpl.is_empty() {
            self.templates.default_tpl.as_str()
        } else {
            "%parents%%line% %action%"
        }
    }

    fn merge_overlay(&mut self, o: ThemeOverlay) {
        if let Some(s) = o.parent {
            self.parent = s;
        }
        if let Some(s) = o.bracket {
            self.bracket = s;
        }
        if let Some(s) = o.project {
            self.project = s;
        }
        if let Some(s) = o.action {
            self.action = s;
        }
        if let Some(s) = o.tags {
            self.tags = s;
        }
        if let Some(s) = o.value_parens {
            self.value_parens = s;
        }
        if let Some(s) = o.values {
            self.values = s;
        }
        if let Some(s) = o.filename {
            self.filename = s;
        }
        if let Some(s) = o.line {
            self.line = s;
        }
        if let Some(s) = o.note {
            self.note = s;
        }
        if let Some(t) = o.templates {
            if let Some(s) = t.default_tpl {
                self.templates.default_tpl = s;
            }
            if let Some(s) = t.single_file {
                self.templates.single_file = s;
            }
            if let Some(s) = t.multi_file {
                self.templates.multi_file = s;
            }
            if let Some(s) = t.no_file {
                self.templates.no_file = s;
            }
            if let Some(s) = t.output {
                self.templates.output = s;
            }
        }
    }

    /// Merge defaults with theme files, in order (later files override):
    /// 1. `~/.local/share/na/theme.yaml` (Ruby gem layout)
    /// 2. XDG data dir `na/theme.yaml` (see [`na_data_dir`])
    pub fn load() -> Self {
        let mut theme = Theme::default();
        for path in theme_candidate_paths() {
            if path.exists() {
                if let Ok(s) = fs::read_to_string(&path) {
                    if let Ok(overlay) = serde_yaml::from_str::<ThemeOverlay>(&s) {
                        theme.merge_overlay(overlay);
                    }
                }
            }
        }
        theme
    }
}

fn theme_candidate_paths() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(home) = env::var("HOME") {
        let h = PathBuf::from(home);
        v.push(h.join(".local/share/na/theme.yaml"));
    }
    v.push(na_data_dir().join("theme.yaml"));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_partial_overrides() {
        let mut t = Theme::default();
        t.merge_overlay(ThemeOverlay {
            action: Some("{r}".to_string()),
            ..Default::default()
        });
        assert_eq!(t.action, "{r}");
        assert_eq!(t.parent, "{bc}");
    }

    #[test]
    fn yaml_overlay_parses_braced_colors() {
        let yaml = r#"action: "{m}""#;
        let overlay: ThemeOverlay = serde_yaml::from_str(yaml).expect("parse");
        let mut t = Theme::default();
        t.merge_overlay(overlay);
        assert_eq!(t.action, "{m}");
    }

    #[test]
    fn flat_action_template_prefers_single_file_when_set() {
        let mut t = Theme::default();
        t.templates.default_tpl = "%action%".to_string();
        t.templates.single_file = "%line%".to_string();
        assert_eq!(t.flat_action_template(false, false), "%line%");
    }

    #[test]
    fn templates_output_overrides_mode_templates() {
        let mut t = Theme::default();
        t.templates.output = "%action% OUT".to_string();
        t.templates.single_file = "%line%".to_string();
        t.templates.multi_file = "%filename%".to_string();
        assert_eq!(t.flat_action_template(false, false), "%action% OUT");
        assert_eq!(t.flat_action_template(true, false), "%action% OUT");
    }

    #[test]
    fn flat_action_template_multi_file_fallback() {
        let mut t = Theme::default();
        t.templates.multi_file.clear();
        assert_eq!(
            t.flat_action_template(true, false),
            "%filename%%line%%parents% %action%"
        );
    }

    #[test]
    fn omit_filename_selects_no_file_template() {
        let mut t = Theme::default();
        t.templates.no_file = "%line%%action%".to_string();
        assert_eq!(t.flat_action_template(false, true), "%line%%action%");
    }

    #[test]
    fn omit_filename_ignores_no_file_when_empty() {
        let mut t = Theme::default();
        t.templates.no_file.clear();
        assert_eq!(
            t.flat_action_template(false, true),
            t.templates.default_tpl.as_str()
        );
    }
}
