use crate::cli::Cli;
use anyhow::{Context, Result};
use inquire::Confirm;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;

/// When `--repo-top` is set, use `{git_root}/{repo_name}.{ext}` as the global todo file.
pub fn apply_repo_top(cli: &mut Cli) -> Result<()> {
    if !cli.repo_top {
        return Ok(());
    }
    let Some(root) = git_repo_root()? else {
        return Ok(());
    };
    let repo_name = root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("repo");
    let taskpaper = root.join(format!("{repo_name}.{}", cli.extension));
    if !taskpaper.is_file() {
        let create = confirm_create(&format!(
            "Repository file not found, create {}?",
            taskpaper.display()
        ))?;
        if !create {
            anyhow::bail!("Cancelled");
        }
        create_todo(
            &taskpaper,
            repo_name,
            cli.template.as_deref(),
            cli.na_tag.trim_start_matches('@'),
        )?;
    }
    cli.global_file = Some(taskpaper);
    Ok(())
}

pub fn git_repo_root() -> Result<Option<PathBuf>> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("Failed to run git")?;
    if !output.status.success() {
        return Ok(None);
    }
    let root = String::from_utf8(output.stdout)?.trim().to_string();
    if root.is_empty() {
        Ok(None)
    } else {
        Ok(Some(PathBuf::from(root)))
    }
}

/// Basename for a new todo file: git repo top-level directory name when available,
/// otherwise the current working directory name (Ruby cwd basename).
pub fn default_todo_basename() -> Result<String> {
    if let Some(root) = git_repo_root()? {
        if let Some(name) = root.file_name().and_then(|s| s.to_str()) {
            if !name.is_empty() {
                return Ok(name.to_string());
            }
        }
    }
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    Ok(cwd
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("todo")
        .to_string())
}

/// Path and basename for a newly created local todo when none exists.
/// Prefers `{git_root}/{repo_name}.{ext}` inside a git repo; otherwise `{cwd}/{cwd_name}.{ext}`.
pub fn default_new_todo_target(extension: &str) -> Result<(PathBuf, String)> {
    let basename = default_todo_basename()?;
    if let Some(root) = git_repo_root()? {
        return Ok((
            root.join(format!("{basename}.{extension}")),
            basename,
        ));
    }
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    Ok((cwd.join(format!("{basename}.{extension}")), basename))
}

/// Prompt (TTY) or auto-accept (non-TTY) whether to create a missing todo file.
pub fn confirm_create(prompt: &str) -> Result<bool> {
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        Ok(Confirm::new(prompt)
            .with_default(true)
            .prompt()
            .unwrap_or(true))
    } else {
        Ok(true)
    }
}

/// Write a new todo file using `--template` when it points at an existing file,
/// otherwise the built-in Ruby-compatible blank template.
pub fn create_todo(
    path: &Path,
    basename: &str,
    template: Option<&str>,
    na_tag: &str,
) -> Result<()> {
    let content = if let Some(template_path) = template.filter(|t| !t.is_empty()) {
        let tpl = Path::new(template_path);
        if tpl.is_file() {
            fs::read_to_string(tpl).with_context(|| format!("Failed to read template {:?}", tpl))?
        } else {
            default_todo_content(basename, na_tag)
        }
    } else {
        default_todo_content(basename, na_tag)
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("Failed to create {:?}", parent))?;
    }
    // Ensure trailing newline like Ruby `puts`.
    let body = if content.ends_with('\n') {
        content
    } else {
        format!("{content}\n")
    };
    fs::write(path, body).with_context(|| format!("Failed to write {:?}", path))?;
    eprintln!("Created {}", path.display());
    Ok(())
}

pub fn default_todo_content(basename: &str, na_tag: &str) -> String {
    let tag = na_tag.trim_start_matches('@');
    format!(
        "Inbox:\n\
{basename}:\n\
\tFeature Requests:\n\
\tIdeas:\n\
\tBugs:\n\
Archive:\n\
Search Definitions:\n\
\tTop Priority @search(@priority = 5 and not @done)\n\
\tHigh Priority @search(@priority > 3 and not @done)\n\
\tMaybe @search(@maybe)\n\
\tNext @search(@{tag} and not @done and not project = \"Archive\")\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_todo_includes_basename_project_and_na_tag() {
        let body = default_todo_content("myrepo", "na");
        assert!(body.contains("myrepo:"));
        assert!(body.contains("Inbox:"));
        assert!(body.contains("@search(@na and not @done"));
        assert!(body.contains("Feature Requests:"));
    }

    #[test]
    fn default_todo_respects_custom_na_tag() {
        let body = default_todo_content("proj", "next");
        assert!(body.contains("@search(@next and not @done"));
    }
}
