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
        let create = if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
            Confirm::new(&format!(
                "Repository file not found, create {}?",
                taskpaper.display()
            ))
            .with_default(true)
            .prompt()
            .unwrap_or(true)
        } else {
            true
        };
        if !create {
            anyhow::bail!("Cancelled");
        }
        create_repo_todo(&taskpaper, repo_name, cli.template.as_deref())?;
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

fn create_repo_todo(path: &Path, basename: &str, template: Option<&str>) -> Result<()> {
    let content = if let Some(template_path) = template.filter(|t| !t.is_empty()) {
        let tpl = Path::new(template_path);
        if tpl.is_file() {
            fs::read_to_string(tpl).with_context(|| format!("Failed to read template {:?}", tpl))?
        } else {
            default_repo_todo_content(basename)
        }
    } else {
        default_repo_todo_content(basename)
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("Failed to create {:?}", parent))?;
    }
    fs::write(path, content).with_context(|| format!("Failed to write {:?}", path))?;
    eprintln!("Created {}", path.display());
    Ok(())
}

fn default_repo_todo_content(basename: &str) -> String {
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
\tNext @search(@na and not @done and not project = \"Archive\")\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_repo_todo_includes_basename_project() {
        let body = default_repo_todo_content("myrepo");
        assert!(body.contains("myrepo:"));
        assert!(body.contains("Inbox:"));
    }
}
