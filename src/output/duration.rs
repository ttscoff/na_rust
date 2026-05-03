//! Duration formatting aligned with Ruby `NA::Actions#format_duration` and time-summary output.

use crate::models::action::Action;
use crate::parser::parse_tag_datetime;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;

/// Elapsed interval from `@started`/`@start` through `@done` (non-negative seconds only).
pub(crate) fn action_timing_window(action: &Action) -> Option<(DateTime<Utc>, DateTime<Utc>, i64)> {
    let started_raw = action
        .tag_value("started")
        .or_else(|| action.tag_value("start"))?;
    let done_raw = action.tag_value("done")?;
    let started = parse_tag_datetime(started_raw)?;
    let ended = parse_tag_datetime(done_raw)?;
    let secs = (ended - started).num_seconds();
    if secs >= 0 {
        Some((started, ended, secs))
    } else {
        None
    }
}

#[inline]
pub(crate) fn action_elapsed_seconds(action: &Action) -> Option<i64> {
    action_timing_window(action).map(|(_, _, s)| s)
}

/// Mirror Ruby `NA::Actions#format_duration`: `DD:HH:MM:SS` or comma-separated English units.
pub(crate) fn format_ruby_duration(secs: i64, human: bool) -> String {
    if secs < 0 {
        return String::new();
    }
    let secs = secs as u64;
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let hours = rem / 3600;
    let rem = rem % 3600;
    let minutes = rem / 60;
    let seconds = rem % 60;
    if human {
        let mut parts = Vec::new();
        if days > 0 {
            parts.push(format!("{days} days"));
        }
        if hours > 0 {
            parts.push(format!("{hours} hours"));
        }
        if minutes > 0 {
            parts.push(format!("{minutes} minutes"));
        }
        if seconds > 0 || parts.is_empty() {
            parts.push(format!("{seconds} seconds"));
        }
        parts.join(", ")
    } else {
        format!("{days:02}:{hours:02}:{minutes:02}:{seconds:02}")
    }
}

/// Add [`secs`] to every non-time tag on [`action`] (Ruby `totals_by_tag` semantics).
pub(crate) fn accumulate_timing_totals_by_tag(action: &Action, secs: i64, out: &mut HashMap<String, i64>) {
    for raw in action.tags.iter().map(|t| t.as_str()) {
        let tag = raw.trim_start_matches('@');
        let key = tag.to_ascii_lowercase();
        if matches!(key.as_str(), "start" | "started" | "done") {
            continue;
        }
        let base = tag.find('(').map(|i| &tag[..i]).unwrap_or(tag);
        let base_key = base.to_ascii_lowercase();
        *out.entry(base_key).or_insert(0) += secs;
    }
}

#[derive(Serialize)]
struct JsonTimedRow<'a> {
    action: &'a str,
    started: String,
    ended: String,
    duration: i64,
}

#[derive(Serialize)]
struct JsonTagRow {
    tag: String,
    duration: i64,
}

#[derive(Serialize)]
struct JsonTotalPayload {
    seconds: i64,
    timestamp: String,
    human: String,
}

#[derive(Serialize)]
struct JsonTimesRoot<'a> {
    timed: Vec<JsonTimedRow<'a>>,
    tags: Vec<JsonTagRow>,
    total: JsonTotalPayload,
}

/// Serialize Ruby-shaped `--json-times` payload (pretty-printed JSON).
pub(crate) fn serialize_json_times(
    actions: &[Action],
    totals_by_tag: &HashMap<String, i64>,
    total_seconds: i64,
) -> anyhow::Result<String> {
    let mut timed = Vec::new();
    for action in actions {
        if let Some((st, en, secs)) = action_timing_window(action) {
            timed.push(JsonTimedRow {
                action: action.text.as_str(),
                started: st.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                ended: en.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                duration: secs,
            });
        }
    }
    let mut tag_rows: Vec<(String, i64)> = totals_by_tag
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    tag_rows.sort_by(|a, b| b.1.cmp(&a.1));
    let tags: Vec<JsonTagRow> = tag_rows
        .into_iter()
        .map(|(k, d)| JsonTagRow {
            tag: k,
            duration: d,
        })
        .collect();
    let root = JsonTimesRoot {
        timed,
        tags,
        total: JsonTotalPayload {
            seconds: total_seconds,
            timestamp: format_ruby_duration(total_seconds, false),
            human: format_ruby_duration(total_seconds, true),
        },
    };
    Ok(serde_json::to_string_pretty(&root)?)
}

/// Footer: either a single "Total time" line or a Markdown table per tag + total row.
pub(crate) fn render_duration_footer(stdout: &mut impl std::fmt::Write, total_seconds: i64, human: bool, totals_by_tag: &HashMap<String, i64>) -> std::fmt::Result {
    if total_seconds <= 0 {
        return Ok(());
    }
    let total_disp = format_ruby_duration(total_seconds, human);
    writeln!(stdout)?;
    if totals_by_tag.is_empty() {
        writeln!(stdout, "Total time: [{total_disp}]")?;
        return Ok(());
    }
    let mut tag_pairs: Vec<(&String, &i64)> = totals_by_tag.iter().collect();
    tag_pairs.sort_by(|a, b| b.1.cmp(a.1));
    let rows: Vec<(String, String)> = tag_pairs
        .into_iter()
        .map(|(tag, secs)| (format!("@{tag}"), format_ruby_duration(*secs, human)))
        .collect();
    let tag_header = "Tag";
    let dur_header = if human {
        "Duration (human)"
    } else {
        "Duration"
    };
    let tag_width = vec![tag_header.len(), "Total".len()]
        .into_iter()
        .chain(rows.iter().map(|(t, _)| t.len()))
        .max()
        .unwrap_or(5);
    let dur_width = vec![dur_header.len(), total_disp.len()]
        .into_iter()
        .chain(rows.iter().map(|(_, d)| d.len()))
        .max()
        .unwrap_or(10);
    writeln!(
        stdout,
        "| {:<lw$} | {:<rw$} |",
        tag_header,
        dur_header,
        lw = tag_width,
        rw = dur_width
    )?;
    writeln!(
        stdout,
        "| {} | {} |",
        "-".repeat(tag_width),
        "-".repeat(dur_width)
    )?;
    for (tag, disp) in &rows {
        writeln!(
            stdout,
            "| {:<lw$} | {:<rw$} |",
            tag,
            disp,
            lw = tag_width,
            rw = dur_width
        )?;
    }
    writeln!(
        stdout,
        "| {} | {} |",
        "=".repeat(tag_width),
        "=".repeat(dur_width)
    )?;
    writeln!(
        stdout,
        "| {:<lw$} | {:<rw$} |",
        "Total",
        total_disp,
        lw = tag_width,
        rw = dur_width
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ninety_minutes_compact_format() {
        assert_eq!(
            format_ruby_duration(90 * 60, false),
            "00:01:30:00"
        );
    }

    #[test]
    fn human_zero_seconds_mirror_ruby_plural_quirk() {
        assert_eq!(format_ruby_duration(0, true), "0 seconds");
    }

    #[test]
    fn human_mirror_ruby_singular_plural_style() {
        let s = 3600 + 60 + 1;
        assert_eq!(
            format_ruby_duration(s, true),
            "1 hours, 1 minutes, 1 seconds"
        );
    }
}
