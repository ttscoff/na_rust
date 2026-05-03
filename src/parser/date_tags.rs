//! Natural-language expansion inside `@tag(...)` parentheses for configured date-ish tags.

use std::sync::OnceLock;

use chrono::{DateTime, Duration, TimeZone, Utc};
use interim::{parse_date_string, parse_duration, Dialect, Interval};
use regex::Regex;

use super::datetime::parse_tag_datetime;

static TAG_DATE_RX: OnceLock<Regex> = OnceLock::new();

fn tag_date_regex() -> &'static Regex {
    TAG_DATE_RX.get_or_init(|| {
        Regex::new(r#"(?is)(?P<pre>(?:^|[ \t]))@(?P<tag>due|start(?:ed)?|beg[ia]n|done|finished|complete[d]?|waiting|defer(?:red)?)\((?P<date>[^)]*)\)"#)
            .expect("tag date regex")
    })
}

fn iso_rx_matches(s: &str) -> bool {
    static RX: OnceLock<Regex> = OnceLock::new();
    RX.get_or_init(|| Regex::new(r"\d{4}-\d\d-\d\d \d\d:\d\d").expect("iso rx"))
        .is_match(s)
}

static EXPLICIT_PAST_RX: OnceLock<Regex> = OnceLock::new();

fn explicit_past_phrase(s: &str) -> bool {
    EXPLICIT_PAST_RX
        .get_or_init(|| Regex::new(r"(?is)\bago\b|yesterday|\blast\b").expect("past rx"))
        .is_match(s)
}

fn is_doneish_tag(tag: &str) -> bool {
    let t = tag.to_ascii_lowercase();
    t.starts_with("done") || t.starts_with("complete")
}

/// Ruby `String#expand_date_tags` — expand natural language inside `@due(...)`, `@started(...)`,
/// `@done(...)`, etc. On failure preserves the original token.
pub(crate) fn expand_date_tags_in_line(input: &str) -> String {
    let now = Utc::now();
    let rx = tag_date_regex();
    let mut out = String::with_capacity(input.len());
    let mut last = 0usize;
    for cap in rx.captures_iter(input) {
        let m = cap.get(0).expect("full match");
        out.push_str(&input[last..m.start()]);
        last = m.end();

        let pre = cap.name("pre").map(|m| m.as_str()).unwrap_or("");
        let tag = cap.name("tag").map(|m| m.as_str()).unwrap_or("");
        let date_inner = cap.name("date").map(|m| m.as_str()).unwrap_or("");

        let expanded = expand_one_tag_value(tag, date_inner, now);
        let replacement = match expanded {
            Some(dt) => format!("{pre}@{tag}({})", dt.format("%Y-%m-%d %H:%M")),
            None => m.as_str().to_string(),
        };
        out.push_str(&replacement);
    }
    out.push_str(&input[last..]);
    out
}

fn expand_one_tag_value(tag: &str, date_inner: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let d = date_inner.trim();
    if d.is_empty() {
        return None;
    }

    let future = if is_doneish_tag(tag) {
        false
    } else {
        !explicit_past_phrase(d)
    };

    if iso_rx_matches(d) {
        if let Some(dt) = parse_tag_datetime(d) {
            return Some(dt);
        }
        return parse_date_string(d, now, Dialect::Us).ok();
    }

    chronify_like_ruby(d, future, now)
}

/// Subset of Ruby `String#chronify` plus `interim` for English phrases.
fn chronify_like_ruby(d: &str, future: bool, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if d.eq_ignore_ascii_case("now") {
        return Some(now);
    }

    if let Ok(minutes) = d.parse::<u64>() {
        return now.checked_sub_signed(Duration::minutes(minutes as i64));
    }

    // `1d2h30m` style — interpreted as elapsed seconds before `now`, like Ruby chronify.
    if let Some(secs_ago) = parse_dhm_secs_ago(d) {
        return now.checked_sub_signed(Duration::seconds(secs_ago));
    }

    // `two hours ago` / `15m ago` → interim relative interval
    if let Ok(iv) = parse_duration(d) {
        return interval_to_datetime(iv, now);
    }

    let dialect = if future { Dialect::Us } else { Dialect::Uk };
    parse_date_string(d, now, dialect).ok()
}

fn parse_dhm_secs_ago(s: &str) -> Option<i64> {
    static DHM_RX: OnceLock<Regex> = OnceLock::new();
    let re = DHM_RX.get_or_init(|| {
        Regex::new(r"(?is)^(?:(?P<day>\d+)d)?\s*(?:(?P<hour>\d+)h)?\s*(?:(?P<min>\d+)m)?\s*$")
            .expect("dhm")
    });
    let cap = re.captures(s)?;
    let day_s = cap.name("day").map(|x| x.as_str()).unwrap_or("");
    let hour_s = cap.name("hour").map(|x| x.as_str()).unwrap_or("");
    let min_s = cap.name("min").map(|x| x.as_str()).unwrap_or("");
    if day_s.is_empty() && hour_s.is_empty() && min_s.is_empty() {
        return None;
    }
    let day: i64 = if day_s.is_empty() {
        0
    } else {
        day_s.parse().ok()?
    };
    let hour: i64 = if hour_s.is_empty() {
        0
    } else {
        hour_s.parse().ok()?
    };
    let min: i64 = if min_s.is_empty() {
        0
    } else {
        min_s.parse().ok()?
    };
    Some(day * 86400 + hour * 3600 + min * 60)
}

fn interval_to_datetime(iv: Interval, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    match iv {
        Interval::Seconds(s) => now.checked_add_signed(Duration::seconds(s as i64)),
        Interval::Days(d) => {
            let date = now
                .date_naive()
                .checked_add_signed(Duration::days(d as i64))?;
            let nn = date.and_time(now.time());
            Some(Utc.from_utc_datetime(&nn))
        }
        Interval::Months(m) => {
            let naive = now.date_naive();
            let date = if m >= 0 {
                naive.checked_add_months(chrono::Months::new(m as u32))?
            } else {
                naive.checked_sub_months(chrono::Months::new((-m) as u32))?
            };
            let nn = date.and_time(now.time());
            Some(Utc.from_utc_datetime(&nn))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_started_done_like_ruby_test() {
        let s = "Task @started(2 hours ago) @done(now)";
        let out = expand_date_tags_in_line(s);
        assert!(Regex::new(r"@started\(\d{4}-\d{2}-\d{2} \d{2}:\d{2}\)")
            .unwrap()
            .is_match(&out));
        assert!(Regex::new(r"@done\(\d{4}-\d{2}-\d{2} \d{2}:\d{2}\)")
            .unwrap()
            .is_match(&out));
    }

    #[test]
    fn preserves_unknown_inner() {
        let s = "x @due(nope_not_a_date_xyz) y";
        assert_eq!(expand_date_tags_in_line(s), s);
    }

    #[test]
    fn iso_inner_normalized() {
        let s = "@done(2020-05-01 14:30)";
        let out = expand_date_tags_in_line(s);
        assert!(out.contains("@done(2020-05-01 14:30)"), "{}", out);
    }
}
