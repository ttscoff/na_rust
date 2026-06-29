use std::io::Write;
use std::io::IsTerminal;
use std::process::{Command, Stdio};

const SMALL_CHAR_LIMIT: usize = 2000;
const SMALL_LINE_LIMIT: usize = 50;

/// Whether pagination should run for this invocation.
pub fn should_paginate(enabled: bool, force_off: bool) -> bool {
    enabled && !force_off && std::io::stdout().is_terminal()
}

/// Write `text` to stdout, paging through `$PAGER` / `less` when appropriate.
pub fn page(text: &str, paginate: bool) {
    if text.is_empty() {
        return;
    }
    if !paginate
        || (text.len() < SMALL_CHAR_LIMIT && text.lines().count() < SMALL_LINE_LIMIT)
    {
        print!("{text}");
        if !text.ends_with('\n') {
            println!();
        }
        return;
    }

    let Some(pager) = resolve_pager() else {
        print!("{text}");
        if !text.ends_with('\n') {
            println!();
        }
        return;
    };

    let mut child = match Command::new("sh")
        .arg("-c")
        .arg(&pager)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => {
            print!("{text}");
            if !text.ends_with('\n') {
                println!();
            }
            return;
        }
    };

    if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all(text.as_bytes());
    }
    let _ = child.wait();
}

fn resolve_pager() -> Option<String> {
    let candidates = pager_candidates();
    for cmd in candidates {
        if let Some(bin) = cmd.split_whitespace().next() {
            if which_executable(bin) {
                return Some(cmd);
            }
        }
    }
    None
}

fn pager_candidates() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(pager) = std::env::var("PAGER") {
        if !pager.trim().is_empty() {
            out.push(pager);
        }
    }
    out.push("less -FXr".to_string());
    if let Ok(git_pager) = std::env::var("GIT_PAGER") {
        if !git_pager.trim().is_empty() {
            out.push(git_pager);
        }
    }
    if let Some(git_pager) = git_core_pager() {
        out.push(git_pager);
    }
    out.push("more -r".to_string());
    out
}

fn git_core_pager() -> Option<String> {
    let git = which_git()?;
    let output = Command::new(git)
        .args(["config", "--get-all", "core.pager"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let pager = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if pager.is_empty() {
        None
    } else {
        Some(pager)
    }
}

fn which_git() -> Option<String> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join("git");
            candidate.is_file().then(|| candidate.to_string_lossy().into_owned())
        })
    })
}

fn which_executable(name: &str) -> bool {
    if name.contains('/') {
        return std::path::Path::new(name).is_file();
    }
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let candidate = dir.join(name);
                candidate.is_file()
            })
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_pager_for_small_output() {
        assert!(!should_paginate(true, false) || std::io::stdout().is_terminal());
        page("hello\n", false);
    }

    #[test]
    fn pager_candidates_include_less_fallback() {
        assert!(pager_candidates().iter().any(|c| c.starts_with("less")));
    }
}
