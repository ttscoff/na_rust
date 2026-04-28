use crate::io::fs::backup_path;
use crate::models::action::Action;
use crate::parser::taskpaper::{extract_actions, render_lines};
use anyhow::{Context, Result};
use chrono::Local;
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(test)]
use crate::parser::search::Query;

#[derive(Debug, Clone)]
pub struct TodoFile {
    pub path: PathBuf,
    lines: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateMutation {
    pub add_tags: Vec<String>,
    pub remove_tags: Vec<String>,
    pub done: bool,
    pub replace_text: Option<String>,
    pub move_to_project: Option<String>,
    pub delete: bool,
    pub restore: bool,
    pub note_lines: Vec<String>,
    pub overwrite_notes: bool,
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

    pub fn project_paths(&self) -> Vec<String> {
        let mut out = Vec::<String>::new();
        for action in self.actions() {
            for i in 0..action.project_chain.len() {
                let path = action.project_chain[..=i].join(":");
                if !out.iter().any(|p| p == &path) {
                    out.push(path);
                }
            }
        }
        out
    }

    pub fn add_action(
        &mut self,
        project: Option<&str>,
        text: &str,
        notes: &[String],
        append: bool,
    ) {
        let project_name = project
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Inbox");
        let header = format!("{project_name}:");
        let proj_idx = self
            .lines
            .iter()
            .position(|line| line.trim() == header)
            .unwrap_or_else(|| {
                if !self.lines.is_empty() && !self.lines.last().is_some_and(|l| l.trim().is_empty()) {
                    self.lines.push(String::new());
                }
                self.lines.push(header.clone());
                self.lines.len() - 1
            });

        let insert_idx = if append {
            project_block_end(&self.lines, proj_idx)
        } else {
            proj_idx + 1
        };

        self.lines.insert(insert_idx, format!("- {text}"));
        for (i, note) in notes.iter().enumerate() {
            self.lines.insert(insert_idx + 1 + i, format!("\t{note}"));
        }
    }

    #[cfg(test)]
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
                if done {
                    apply_done_timestamp(line);
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

    pub fn apply_update_by_lines(
        &mut self,
        line_indices: &HashSet<usize>,
        tags: &[String],
        remove_tags: &[String],
        done: bool,
    ) -> Result<usize> {
        let mutation = UpdateMutation {
            add_tags: tags.to_vec(),
            remove_tags: remove_tags.to_vec(),
            done,
            ..UpdateMutation::default()
        };
        self.apply_mutation_by_lines(line_indices, &mutation)
    }

    pub fn apply_mutation_by_lines(
        &mut self,
        line_indices: &HashSet<usize>,
        mutation: &UpdateMutation,
    ) -> Result<usize> {
        let mut changed = 0usize;
        let mut actions = self.actions();
        actions.sort_by(|a, b| b.line_index.cmp(&a.line_index));
        for action in actions {
            if !line_indices.contains(&action.line_index) {
                continue;
            }
            let start = action.line_index;
            let end = (start + 1 + action.notes.len()).min(self.lines.len());
            if start >= self.lines.len() {
                continue;
            }
            if mutation.delete {
                self.lines.drain(start..end);
                changed += 1;
                continue;
            }

            let mut text = mutation
                .replace_text
                .clone()
                .unwrap_or_else(|| action.text.clone());
            if mutation.done {
                text = apply_done_timestamp_to_text(&text);
            }
            for tag in &mutation.add_tags {
                if !text.contains(tag) {
                    text.push(' ');
                    text.push_str(tag);
                }
            }
            for tag in &mutation.remove_tags {
                remove_tag_from_line(&mut text, tag);
            }
            let mut project = action.project.clone().unwrap_or_else(|| "Inbox".to_string());
            if mutation.restore
                && action
                    .project_chain
                    .iter()
                    .any(|p| p.eq_ignore_ascii_case("archive"))
            {
                project = "Inbox".to_string();
            }
            if let Some(to) = &mutation.move_to_project {
                project = to.clone();
            }
            let mut notes = if mutation.overwrite_notes {
                mutation.note_lines.clone()
            } else {
                let mut merged = action.notes.clone();
                merged.extend(mutation.note_lines.clone());
                merged
            };
            notes.retain(|n| !n.trim().is_empty());

            self.lines.drain(start..end);
            self.add_action(Some(&project), &text, &notes, true);
            changed += 1;
        }
        if changed > 0 {
            self.save()?;
        }
        Ok(changed)
    }

    pub fn archive_actions_by_lines(&mut self, line_indices: &HashSet<usize>) -> Result<usize> {
        let mut selected: Vec<Action> = self
            .actions()
            .into_iter()
            .filter(|a| line_indices.contains(&a.line_index))
            .collect();
        if selected.is_empty() {
            return Ok(0);
        }
        selected.sort_by_key(|a| a.line_index);
        let mut archived_items: Vec<(String, Vec<String>)> = Vec::new();
        for action in &selected {
            let mut text = action.text.clone();
            if !action.done {
                text = apply_done_timestamp_to_text(&text);
            }
            archived_items.push((text, action.notes.clone()));
        }

        // Remove original action blocks bottom-up.
        for action in selected.iter().rev() {
            let start = action.line_index;
            let end = start + 1 + action.notes.len();
            if start < self.lines.len() {
                let capped_end = end.min(self.lines.len());
                self.lines.drain(start..capped_end);
            }
        }

        for (text, notes) in archived_items {
            self.add_action(Some("Archive"), &text, &notes, true);
        }
        self.save()?;
        Ok(line_indices.len())
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

fn apply_done_timestamp(line: &mut String) {
    *line = apply_done_timestamp_to_text(line);
}

fn apply_done_timestamp_to_text(text: &str) -> String {
    let mut line = text.to_string();
    remove_tag_from_line(&mut line, "@done");
    let stamp = Local::now().format("%Y-%m-%d %H:%M").to_string();
    if line.trim().is_empty() {
        format!("@done({stamp})")
    } else {
        format!("{} @done({stamp})", line.trim())
    }
}

fn project_block_end(lines: &[String], project_idx: usize) -> usize {
    let project_indent = leading_whitespace(lines[project_idx].as_str());
    let mut idx = project_idx + 1;
    while idx < lines.len() {
        let line = lines[idx].as_str();
        let trimmed = line.trim();
        if !trimmed.is_empty() && trimmed.ends_with(':') && leading_whitespace(line) <= project_indent {
            break;
        }
        idx += 1;
    }
    idx
}

fn leading_whitespace(line: &str) -> usize {
    line.chars().take_while(|c| c.is_ascii_whitespace()).count()
}

#[cfg(test)]
mod tests {
    use super::TodoFile;
    use crate::io::fs::backup_path;
    use crate::parser::search::Query;
    use std::collections::HashSet;
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
        assert!(updated.contains("- Task one @done("), "{updated}");
        assert!(updated.contains("@today"), "{updated}");
        assert!(updated.contains("- Task two @done\n"));
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

    #[test]
    fn add_action_inserts_at_project_start_or_end() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Existing A
- Existing B
"#,
        );
        let mut todo = TodoFile::load(&path).expect("fixture should load");
        todo.add_action(Some("Work"), "First", &[String::from("note one")], false);
        todo.add_action(Some("Work"), "Last", &[], true);
        let backup = backup_path(&path);
        todo.save().expect("save should work");
        let updated = fs::read_to_string(&path).expect("updated file should read");
        fs::remove_file(path).ok();
        fs::remove_file(backup).ok();

        assert!(updated.contains("Work:\n- First\n\tnote one\n- Existing A\n- Existing B\n- Last"));
    }

    #[test]
    fn apply_update_by_lines_updates_only_selected_actions() {
        let path = write_fixture_taskpaper(
            r#"Work:
- One
- Two
"#,
        );
        let mut todo = TodoFile::load(&path).expect("fixture should load");
        let selected = HashSet::from([2usize]);
        let backup = backup_path(&path);
        let changed = todo
            .apply_update_by_lines(&selected, &["@today".to_string()], &[], true)
            .expect("update should apply");
        let updated = fs::read_to_string(&path).expect("updated file should read");
        fs::remove_file(path).ok();
        fs::remove_file(backup).ok();
        assert_eq!(changed, 1);
        assert!(updated.contains("- Two @done("), "{updated}");
        assert!(updated.contains("@today"), "{updated}");
        assert!(updated.contains("- One\n"));
    }

    #[test]
    fn archive_actions_by_lines_moves_selected_to_archive_and_marks_done() {
        let path = write_fixture_taskpaper(
            r#"Work:
- Ship thing @na
- Already done @done(2026-01-01 12:00)
"#,
        );
        let mut todo = TodoFile::load(&path).expect("fixture should load");
        let selected = HashSet::from([1usize, 2usize]);
        let backup = backup_path(&path);
        let moved = todo
            .archive_actions_by_lines(&selected)
            .expect("archive should apply");
        let updated = fs::read_to_string(&path).expect("updated file should read");
        fs::remove_file(path).ok();
        fs::remove_file(backup).ok();
        assert_eq!(moved, 2);
        assert!(updated.contains("Archive:\n- Ship thing @na @done("), "{updated}");
        assert!(updated.contains("- Already done @done(2026-01-01 12:00)"), "{updated}");
        assert!(!updated.contains("Work:\n- Ship thing"), "{updated}");
    }
}
