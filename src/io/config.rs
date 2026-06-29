use crate::cli::Cli;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use super::xdg::config_home;

/// Globals persisted in `na.rc` (Ruby-compatible search paths).
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct NaRcGlobals {
    #[serde(default, alias = "extension")]
    pub ext: Option<String>,
    #[serde(default, alias = "global_file")]
    pub file: Option<String>,
    #[serde(default, alias = "na_tag", alias = "tag")]
    pub na_tag: Option<String>,
    #[serde(default)]
    pub add_at: Option<String>,
    #[serde(default)]
    pub depth: Option<usize>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub no_color: Option<bool>,
    #[serde(default)]
    pub cwd_as: Option<String>,
}

/// Ruby `find_config_file`: `$XDG_CONFIG_HOME/na/na.rc`, `$XDG_CONFIG_HOME/na.rc`, `~/.na.rc`.
pub fn find_na_rc_path() -> PathBuf {
    let xdg = config_home();
    let candidates = [
        xdg.join("na").join("na.rc"),
        xdg.join("na.rc"),
        home_na_rc(),
    ];
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .unwrap_or_else(|| xdg.join("na").join("na.rc"))
}

fn home_na_rc() -> PathBuf {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".na.rc"))
        .unwrap_or_else(|| PathBuf::from(".na.rc"))
}

/// Load globals from the first existing `na.rc`, if any.
pub fn load_na_rc_globals() -> Result<Option<NaRcGlobals>> {
    let path = find_na_rc_path();
    if !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("Failed to read {:?}", path))?;
    Ok(Some(parse_na_rc(&raw)?))
}

fn parse_na_rc(raw: &str) -> Result<NaRcGlobals> {
    let normalized = normalize_gli_yaml(raw);
    if let Ok(cfg) = serde_yaml::from_str::<NaRcGlobals>(&normalized) {
        if cfg != NaRcGlobals::default() || normalized.contains("ext:") {
            return Ok(cfg);
        }
    }
    Ok(parse_na_rc_line_fallback(raw))
}

/// GLI writes `:ext:` symbol keys; normalize to plain YAML keys for `serde_yaml`.
fn normalize_gli_yaml(raw: &str) -> String {
    let mut out = String::new();
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with(':') && trimmed.contains(':') {
            let rest = trimmed.trim_start_matches(':');
            if let Some((key, val)) = rest.split_once(':') {
                let indent = line.len() - trimmed.len();
                out.push_str(&" ".repeat(indent));
                out.push_str(key.trim());
                out.push(':');
                out.push_str(val);
                out.push('\n');
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn parse_na_rc_line_fallback(raw: &str) -> NaRcGlobals {
    let mut cfg = NaRcGlobals::default();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let key = k.trim().trim_start_matches(':');
        let val = v.trim().trim_matches('"').trim_matches('\'');
        match key {
            "ext" | "extension" => cfg.ext = Some(val.to_string()),
            "file" | "global_file" => cfg.file = Some(val.to_string()),
            "na_tag" | "tag" => cfg.na_tag = Some(val.to_string()),
            "add_at" => cfg.add_at = Some(val.to_string()),
            "depth" | "d" => cfg.depth = val.parse().ok(),
            "template" => cfg.template = Some(val.to_string()),
            "no_color" => cfg.no_color = Some(matches!(val, "true" | "yes" | "1")),
            "cwd_as" => cfg.cwd_as = Some(val.to_string()),
            _ => {}
        }
    }
    cfg
}

/// Apply `na.rc` defaults where the CLI did not override (clap defaults only).
pub fn apply_rc_defaults(cli: &mut Cli) {
    let Ok(Some(rc)) = load_na_rc_globals() else {
        return;
    };
    if cli.extension == "taskpaper" {
        if let Some(ext) = rc.ext.filter(|s| !s.is_empty()) {
            cli.extension = ext;
        }
    }
    if cli.global_file.is_none() {
        if let Some(file) = rc.file.filter(|s| !s.is_empty()) {
            cli.global_file = Some(expand_tilde(&file));
        }
    }
    if !cli.no_color {
        if let Some(true) = rc.no_color {
            cli.no_color = true;
        }
    }
    if cli.na_tag == "na" {
        if let Some(tag) = rc.na_tag.filter(|s| !s.is_empty()) {
            cli.na_tag = tag;
        }
    }
    if cli.add_at == "start" {
        if let Some(at) = rc.add_at.filter(|s| !s.is_empty()) {
            cli.add_at = at;
        }
    }
    if cli.depth.is_none() {
        cli.depth = rc.depth;
    }
    if cli.template.is_none() {
        cli.template = rc.template.filter(|s| !s.is_empty());
    }
    if cli.cwd_as == "none" {
        if let Some(cwd_as) = rc.cwd_as.filter(|s| !s.is_empty()) {
            cli.cwd_as = cwd_as;
        }
    }
}

pub fn rc_globals_from_cli(cli: &Cli) -> NaRcGlobals {
    NaRcGlobals {
        ext: Some(cli.extension.clone()),
        file: cli
            .global_file
            .as_ref()
            .map(|p| p.display().to_string()),
        na_tag: Some(cli.na_tag.clone()),
        add_at: Some(cli.add_at.clone()),
        depth: cli.depth,
        template: cli.template.clone(),
        no_color: if cli.no_color { Some(true) } else { None },
        cwd_as: Some(cli.cwd_as.clone()),
    }
}

pub fn write_na_rc(path: &Path, globals: &NaRcGlobals) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("Failed to create {:?}", parent))?;
    }
    let body = serde_yaml::to_string(globals).context("Failed to serialize na.rc")?;
    fs::write(path, body).with_context(|| format!("Failed to write {:?}", path))?;
    Ok(())
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    if path == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_gli_yaml_strips_symbol_colons() {
        let raw = ":ext: md\n:file: ~/t.taskpaper\n";
        let norm = normalize_gli_yaml(raw);
        assert!(norm.contains("ext: md"));
        assert!(norm.contains("file: ~/t.taskpaper"));
    }

    #[test]
    fn parse_gli_style_rc() {
        let raw = ":ext: tp\n:na_tag: next\n";
        let cfg = parse_na_rc(raw).expect("parse");
        assert_eq!(cfg.ext.as_deref(), Some("tp"));
        assert_eq!(cfg.na_tag.as_deref(), Some("next"));
    }

    #[test]
    fn rc_globals_from_cli_roundtrip() {
        let cli = Cli {
            extension: "md".to_string(),
            na_tag: "next".to_string(),
            add_at: "end".to_string(),
            depth: Some(2),
            template: Some("Inbox:".to_string()),
            cwd_as: "project".to_string(),
            no_color: true,
            ..Default::default()
        };
        let globals = rc_globals_from_cli(&cli);
        assert_eq!(globals.ext.as_deref(), Some("md"));
        assert_eq!(globals.na_tag.as_deref(), Some("next"));
        assert_eq!(globals.add_at.as_deref(), Some("end"));
        assert_eq!(globals.depth, Some(2));
        assert_eq!(globals.template.as_deref(), Some("Inbox:"));
        assert_eq!(globals.cwd_as.as_deref(), Some("project"));
        assert_eq!(globals.no_color, Some(true));
    }
}
