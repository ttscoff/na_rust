use crate::io::fs::backup_path;
use crate::models::action::Action;
use crate::parser::search::Query;
use crate::parser::taskpaper::{extract_actions, render_lines};
use anyhow::{Context, Result};
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct TodoFile {
    pub path: PathBuf,
    lines: Vec<String>,
}

impl TodoFile {
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let lines = content.lines().map(ToString::to_string).collect();
        Ok(Self {
            path: path.to_path_buf(),
            lines,
        })
    }

    pub fn actions(&self) -> Vec<Action> {
        extract_actions(&self.lines, &self.path)
    }

    pub fn add_inbox_action(&mut self, text: &str) {
        self.lines.push(format!("- {text}"));
    }

    pub fn apply_update(
        &mut self,
        query: &Query,
        tags: &[String],
        remove_tags: &[String],
        done: bool,
    ) -> Result<usize> {
        let mut changed = 0usize;
        let actions = self.actions();

        for action in actions {
            if !query.matches(&action) {
                continue;
            }

            if let Some(line) = self.lines.get_mut(action.line_index) {
                let before = line.clone();
                if done && !line.contains("@done") {
                    line.push_str(" @done");
                }
                for tag in tags {
                    if !line.contains(tag) {
                        line.push(' ');
                        line.push_str(tag);
                    }
                }
                for tag in remove_tags {
                    remove_tag_from_line(line, tag);
                }
                if *line != before {
                    changed += 1;
                }
            }
        }

        if changed > 0 {
            self.save()?;
        }

        Ok(changed)
    }

    pub fn save(&self) -> Result<()> {
        if self.path.exists() {
            let backup = backup_path(&self.path);
            if let Some(parent) = backup.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed creating backup dir {:?}", parent))?;
            }
            fs::copy(&self.path, &backup).with_context(|| format!("Failed writing backup {:?}", backup))?;
        }
        fs::write(&self.path, render_lines(&self.lines))?;
        Ok(())
    }
}

fn remove_tag_from_line(line: &mut String, tag: &str) {
    let normalized = tag.trim();
    if normalized.is_empty() {
        return;
    }
    let tag_name = normalized.trim_start_matches('@');
    if tag_name.is_empty() {
        return;
    }

    let pattern = format!(r"(?i)\s*@{}(?:\([^)]*\))?", regex::escape(tag_name));
    let re = Regex::new(&pattern).expect("valid tag removal regex");
    let updated = re.replace_all(line, "").to_string();
    let compact = updated.split_whitespace().collect::<Vec<_>>().join(" ");
    *line = compact;
}

#[cfg(test)]
mod tests {
    use super::TodoFile;
    use crate::io::fs::backup_path;
    use crate::parser::search::Query;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn write_fixture_taskpaper(content: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("valid monotonic time")
            .as_nanos();
        let seq = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        path.push(format!(
            "na_rust_update_fixture_{}_{}_{}.taskpaper",
            std::process::id(),
            ts,
            seq
        ));
        fs::write(&path, content).expect("fixture should write");
        path
    }

    #[test]
    fn apply_update_done_and_tag_add_mutates_file_text() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Task one
- Task two @done
"#,
        );
        let mut todo = TodoFile::load(&path).expect("fixture should load");
        let query = Query::parse("@search(not @done and one)").expect("query should parse");
        let backup = backup_path(&path);
        let changed = todo
            .apply_update(&query, &["@today".to_string()], &[], true)
            .expect("update should apply");
        let updated = fs::read_to_string(&path).expect("updated file should read");
        fs::remove_file(path).ok();
        fs::remove_file(backup).ok();

        assert_eq!(changed, 1);
        assert_eq!(
            updated,
            "Work:\n- Task one @done @today\n- Task two @done\n"
        );
    }

    #[test]
    fn apply_update_removes_tags_with_and_without_values() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Keep @home @priority(5) @na
"#,
        );
        let mut todo = TodoFile::load(&path).expect("fixture should load");
        let query = Query::parse("Keep").expect("query should parse");
        let backup = backup_path(&path);
        let changed = todo
            .apply_update(&query, &["@today".to_string()], &["@home".to_string(), "@priority".to_string()], false)
            .expect("update should apply");
        let updated = fs::read_to_string(&path).expect("updated file should read");
        fs::remove_file(path).ok();
        fs::remove_file(backup).ok();

        assert_eq!(changed, 1);
        assert_eq!(updated, "Work:\n- Keep @na @today\n");
    }
}
