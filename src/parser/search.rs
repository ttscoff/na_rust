use crate::models::action::Action;
use crate::models::todo::TodoFile;
use crate::parser::item_path::{parse_item_path, project_chain_matches_path};
use anyhow::Result;
use std::collections::HashSet;

#[derive(Debug, Clone, Default)]
pub struct Query {
    clauses: Vec<Clause>,
    include_done: bool,
    /// Trailing `[n]` or `[a:b]` on the `@search(...)` inner expression (applied per OR clause).
    slice: Option<SearchSlice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SearchSlice {
    Index(isize),
    Range {
        start: Option<usize>,
        end: Option<usize>,
    },
}

#[derive(Debug, Clone, Default)]
struct Clause {
    terms: Vec<String>,
    tags: Vec<String>,
    negated_tags: Vec<String>,
    comparisons: Vec<TagComparison>,
    project: Option<String>,
    exclude_projects: Vec<String>,
    /// Slash item path (`/A/B`, `//X`, `/A/*/B`), same semantics as add/update `project`.
    item_path: Option<String>,
    exclude_item_paths: Vec<String>,
}

#[derive(Debug, Clone)]
struct TagComparison {
    tag: String,
    op: CompareOp,
    value: String,
    negated: bool,
    case_insensitive: bool,
}

#[derive(Debug, Clone, Copy)]
enum CompareOp {
    Eq,
    Gt,
    Lt,
    Ge,
    Le,
    Regex,
    Contains,
    BeginsWith,
    EndsWith,
}

impl Query {
    pub fn parse(input: &str) -> Result<Self> {
        let trimmed = input.trim();
        if trimmed.starts_with("@search(") {
            return Self::parse_taskpaper_search(trimmed);
        }
        let clause = Self::parse_simple_clause(trimmed);
        let include_done = clause.tags.iter().any(|tag| {
            tag.eq_ignore_ascii_case("@done") || tag.to_ascii_lowercase().starts_with("@done(")
        });
        Ok(Self {
            clauses: vec![clause],
            include_done,
            slice: None,
        })
    }

    pub fn next_defaults(na_tag: &str) -> Self {
        Self {
            clauses: vec![Clause {
                tags: vec![format!("@{}", na_tag.trim_start_matches('@'))],
                negated_tags: vec!["@done".to_string()],
                comparisons: Vec::new(),
                project: None,
                exclude_projects: Vec::new(),
                item_path: None,
                exclude_item_paths: Vec::new(),
                terms: Vec::new(),
            }],
            include_done: false,
            slice: None,
        }
    }

    fn parse_simple_clause(input: &str) -> Clause {
        let mut clause = Clause::default();
        for token in input.split_whitespace() {
            if token.starts_with('@') {
                clause.tags.push(token.to_string());
            } else {
                clause.terms.push(token.to_lowercase());
            }
        }
        clause
    }

    fn parse_taskpaper_search(input: &str) -> Result<Self> {
        let mut inner = input.trim();
        if let Some(stripped) = inner
            .strip_prefix("@search(")
            .and_then(|s| s.strip_suffix(')'))
        {
            inner = stripped.trim();
        }

        let (inner, slice) = split_trailing_slice(inner);

        let mut include_done = false;
        let global_excludes = extract_global_not_project(&inner);
        let mut clauses = Vec::new();
        for raw_clause in split_by_keyword(&inner, "or") {
            let raw_clause = strip_wrapping_parens(raw_clause.trim());
            let mut variants = vec![Clause {
                exclude_projects: global_excludes.clone(),
                ..Clause::default()
            }];
            for part in split_by_keyword(raw_clause, "and") {
                let predicate = strip_wrapping_parens(part.trim());
                if predicate.is_empty() {
                    continue;
                }
                let options: Vec<&str> = split_by_keyword(predicate, "or");
                if options.len() <= 1 {
                    for clause in &mut variants {
                        apply_search_predicate(clause, predicate, &mut include_done);
                    }
                    continue;
                }
                let mut expanded = Vec::new();
                for clause in &variants {
                    for option in &options {
                        let mut cloned = clause.clone();
                        apply_search_predicate(
                            &mut cloned,
                            strip_wrapping_parens(option.trim()),
                            &mut include_done,
                        );
                        expanded.push(cloned);
                    }
                }
                variants = expanded;
            }
            clauses.extend(variants);
        }

        Ok(Self {
            clauses,
            include_done,
            slice,
        })
    }

    fn matches_clause(clause: &Clause, action: &Action) -> bool {
        let action_lc = action.text.to_lowercase();
        let terms_ok = clause.terms.iter().all(|term| {
            if term.contains('*') {
                glob_star_ordered_match(&action_lc, term)
            } else {
                action_lc.contains(term)
            }
        });
        let tags_ok = clause.tags.iter().all(|tag| action.has_tag(tag));
        let negated_tags_ok = clause.negated_tags.iter().all(|tag| !action.has_tag(tag));
        let comparisons_ok = clause.comparisons.iter().all(|cmp| {
            let actual = action.tag_value(&cmp.tag);
            let base = compare_tag_value(actual, cmp);
            if cmp.negated {
                !base
            } else {
                base
            }
        });

        let project_names: Vec<&str> = if action.project_chain.is_empty() {
            action
                .project
                .as_deref()
                .map(|p| vec![p])
                .unwrap_or_default()
        } else {
            action.project_chain.iter().map(String::as_str).collect()
        };
        let project_ok = clause
            .project
            .as_ref()
            .is_none_or(|p| project_predicate_matches_include(p.trim(), &project_names));
        let excluded_ok = clause
            .exclude_projects
            .iter()
            .all(|p| !project_predicate_matches_include(p.trim(), &project_names));

        let chain_owned: Vec<String> = project_names.iter().map(|s| (*s).to_string()).collect();
        let item_path_ok = clause
            .item_path
            .as_ref()
            .is_none_or(|path| project_chain_matches_path(&chain_owned, path.trim()));
        let item_path_excludes_ok = clause
            .exclude_item_paths
            .iter()
            .all(|path| !project_chain_matches_path(&chain_owned, path.trim()));

        terms_ok
            && tags_ok
            && negated_tags_ok
            && comparisons_ok
            && project_ok
            && excluded_ok
            && item_path_ok
            && item_path_excludes_ok
    }

    pub fn matches(&self, action: &Action) -> bool {
        self.clauses
            .iter()
            .any(|clause| Self::matches_clause(clause, action))
    }

    pub fn with_include_done(mut self, include_done: bool) -> Self {
        self.include_done = include_done;
        self
    }
}

fn apply_search_predicate(clause: &mut Clause, predicate: &str, include_done: &mut bool) {
    let mut negated = false;
    let mut p = predicate;
    if let Some(rest) = predicate.strip_prefix("not ") {
        negated = true;
        p = rest.trim();
    }
    if let Some(value) = parse_project_predicate(p) {
        if negated {
            clause.exclude_projects.push(value);
        } else {
            clause.project = Some(value);
        }
        return;
    }

    if let Some(path) = parse_item_path_predicate(p) {
        if negated {
            clause.exclude_item_paths.push(path);
        } else {
            clause.item_path = Some(path);
        }
        return;
    }

    if p.starts_with('@') {
        if let Some(comp) = parse_tag_comparison(p, negated) {
            if comp.tag.eq_ignore_ascii_case("done") {
                *include_done = !negated;
            }
            clause.comparisons.push(comp);
            return;
        }
        if p.eq_ignore_ascii_case("@done") {
            *include_done = !negated;
        }
        if negated {
            clause.negated_tags.push(p.to_string());
        } else {
            clause.tags.push(p.to_string());
        }
        return;
    }
    clause.terms.push(strip_quotes(p).to_lowercase());
}

pub fn evaluate_query(files: &[TodoFile], query: &Query) -> Vec<Action> {
    match &query.slice {
        None => files
            .iter()
            .flat_map(TodoFile::actions)
            .filter(|a| (query.include_done || !a.done) && query.matches(a))
            .collect(),
        Some(slice) => {
            let mut seen = HashSet::<String>::new();
            let mut out = Vec::new();
            for clause in &query.clauses {
                let mut matched: Vec<Action> = files
                    .iter()
                    .flat_map(TodoFile::actions)
                    .filter(|a| (query.include_done || !a.done) && Query::matches_clause(clause, a))
                    .collect();
                matched = apply_search_slice(matched, slice);
                for a in matched {
                    let key = format!("{}:{}", a.source_file, a.line_index);
                    if seen.insert(key) {
                        out.push(a);
                    }
                }
            }
            out
        }
    }
}

fn split_trailing_slice(inner: &str) -> (String, Option<SearchSlice>) {
    let trimmed_end = inner.trim_end();
    let Some(without_close) = trimmed_end.strip_suffix(']') else {
        return (inner.to_string(), None);
    };
    let Some(open_bracket) = without_close.rfind('[') else {
        return (inner.to_string(), None);
    };
    let slice_inner = without_close[open_bracket + 1..].trim();
    if !slice_inner.chars().all(|c| c.is_ascii_digit() || c == ':') {
        return (inner.to_string(), None);
    }
    let expr = without_close[..open_bracket].trim_end();
    if expr.is_empty() {
        return (inner.to_string(), None);
    }
    let Some(sl) = parse_slice_inner(slice_inner) else {
        return (inner.to_string(), None);
    };
    (expr.trim().to_string(), Some(sl))
}

fn parse_slice_inner(s: &str) -> Option<SearchSlice> {
    let s = s.trim();
    if s.contains(':') {
        let mut parts = s.splitn(2, ':');
        let a = parts.next().unwrap_or("");
        let b = parts.next().unwrap_or("");
        let start = if a.is_empty() {
            None
        } else {
            Some(a.parse::<usize>().ok()?)
        };
        let end = if b.is_empty() {
            None
        } else {
            Some(b.parse::<usize>().ok()?)
        };
        Some(SearchSlice::Range { start, end })
    } else if s.is_empty() {
        None
    } else {
        Some(SearchSlice::Index(s.parse::<isize>().ok()?))
    }
}

fn apply_search_slice(actions: Vec<Action>, slice: &SearchSlice) -> Vec<Action> {
    match slice {
        SearchSlice::Index(i) => {
            if *i < 0 {
                return Vec::new();
            }
            let i = *i as usize;
            actions.get(i).cloned().into_iter().collect()
        }
        SearchSlice::Range { start, end } => {
            let len = actions.len();
            let s = start.unwrap_or(0).min(len);
            let e = end.unwrap_or(len).min(len);
            if s >= e {
                Vec::new()
            } else {
                actions[s..e].to_vec()
            }
        }
    }
}

fn parse_project_predicate(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let lower = trimmed.to_lowercase();
    if !lower.starts_with("project") {
        return None;
    }
    // Supports both Ruby-like `project Inbox` and explicit `project = "Inbox"`.
    if let Some(rest) = trimmed
        .strip_prefix("project")
        .or_else(|| trimmed.strip_prefix("Project"))
    {
        let rest = rest.trim();
        if !rest.is_empty() && !rest.starts_with('=') {
            return Some(strip_quotes(rest).to_string());
        }
    }
    let parts = trimmed.splitn(2, '=').collect::<Vec<_>>();
    if parts.len() != 2 {
        return None;
    }
    Some(strip_quotes(parts[1].trim()).to_string())
}

fn parse_item_path_predicate(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.starts_with('/') && !parse_item_path(trimmed).is_empty() {
        return Some(trimmed.to_string());
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("path ") {
        let rest = trimmed["path ".len()..].trim();
        if rest.starts_with('/') && !parse_item_path(rest).is_empty() {
            return Some(rest.to_string());
        }
    }
    None
}

fn parse_tag_comparison(input: &str, negated: bool) -> Option<TagComparison> {
    let trimmed = input.trim();
    let no_at = trimmed.strip_prefix('@')?;
    let mut op = None;
    for candidate in [
        "beginswith",
        "endswith",
        "contains",
        "matches",
        "=~",
        ">=",
        "<=",
        "!=",
        "==",
        "=",
        ">",
        "<",
    ] {
        if let Some(pos) = no_at.find(candidate) {
            op = Some((candidate, pos));
            break;
        }
    }
    let (op_str, pos) = op?;
    let tag = no_at[..pos].trim().to_string();
    let mut rest = no_at[pos + op_str.len()..].trim();
    let mut case_insensitive = false;
    if let Some(mod_rest) = rest.strip_prefix("[i]") {
        case_insensitive = true;
        rest = mod_rest.trim();
    }
    let value = strip_quotes(rest).to_string();
    let op = match op_str {
        "=" | "==" | "!=" => CompareOp::Eq,
        ">" => CompareOp::Gt,
        "<" => CompareOp::Lt,
        ">=" => CompareOp::Ge,
        "<=" => CompareOp::Le,
        "=~" | "matches" => CompareOp::Regex,
        "contains" => CompareOp::Contains,
        "beginswith" => CompareOp::BeginsWith,
        "endswith" => CompareOp::EndsWith,
        _ => return None,
    };
    let forced_negate = op_str == "!=";
    Some(TagComparison {
        tag,
        op,
        value,
        negated: negated ^ forced_negate,
        case_insensitive,
    })
}

fn compare_tag_value(actual: Option<&str>, cmp: &TagComparison) -> bool {
    let Some(actual) = actual else { return false };
    match cmp.op {
        CompareOp::Eq => actual.eq_ignore_ascii_case(&cmp.value),
        CompareOp::Contains => {
            if cmp.case_insensitive {
                actual.to_lowercase().contains(&cmp.value.to_lowercase())
            } else {
                actual.contains(&cmp.value)
            }
        }
        CompareOp::BeginsWith => {
            if cmp.case_insensitive {
                actual.to_lowercase().starts_with(&cmp.value.to_lowercase())
            } else {
                actual.starts_with(&cmp.value)
            }
        }
        CompareOp::EndsWith => {
            if cmp.case_insensitive {
                actual.to_lowercase().ends_with(&cmp.value.to_lowercase())
            } else {
                actual.ends_with(&cmp.value)
            }
        }
        CompareOp::Regex => {
            let pattern = if cmp.case_insensitive {
                format!("(?i){}", cmp.value)
            } else {
                cmp.value.clone()
            };
            regex::Regex::new(&pattern)
                .map(|rx| rx.is_match(actual))
                .unwrap_or(false)
        }
        CompareOp::Gt | CompareOp::Lt | CompareOp::Ge | CompareOp::Le => {
            if let (Ok(a), Ok(b)) = (actual.parse::<f64>(), cmp.value.parse::<f64>()) {
                match cmp.op {
                    CompareOp::Gt => a > b,
                    CompareOp::Lt => a < b,
                    CompareOp::Ge => a >= b,
                    CompareOp::Le => a <= b,
                    _ => false,
                }
            } else {
                let ordering = actual.to_lowercase().cmp(&cmp.value.to_lowercase());
                match cmp.op {
                    CompareOp::Gt => ordering.is_gt(),
                    CompareOp::Lt => ordering.is_lt(),
                    CompareOp::Ge => ordering.is_ge(),
                    CompareOp::Le => ordering.is_le(),
                    _ => false,
                }
            }
        }
    }
}

fn strip_quotes(input: &str) -> &str {
    if (input.starts_with('"') && input.ends_with('"'))
        || (input.starts_with('\'') && input.ends_with('\''))
    {
        &input[1..input.len() - 1]
    } else {
        input
    }
}

fn strip_wrapping_parens(input: &str) -> &str {
    let mut out = input.trim();
    loop {
        if !(out.starts_with('(') && out.ends_with(')')) {
            return out;
        }
        let mut quote: Option<char> = None;
        let mut depth = 0usize;
        let mut wraps_whole = true;
        for (i, ch) in out.char_indices() {
            if let Some(q) = quote {
                if ch == q {
                    quote = None;
                }
                continue;
            }
            if ch == '"' || ch == '\'' {
                quote = Some(ch);
                continue;
            }
            if ch == '(' {
                depth += 1;
            } else if ch == ')' {
                if depth == 0 {
                    wraps_whole = false;
                    break;
                }
                depth -= 1;
                if depth == 0 && i + 1 < out.len() {
                    wraps_whole = false;
                    break;
                }
            }
        }
        if wraps_whole {
            out = out[1..out.len() - 1].trim();
        } else {
            return out;
        }
    }
}

fn project_predicate_matches_include(pattern: &str, segments: &[&str]) -> bool {
    if pattern.is_empty() {
        return true;
    }
    if pattern.contains('*') {
        let joined_gt = segments.join(">");
        let joined_colon = segments.join(":");
        glob_star_ordered_match(&joined_gt, pattern)
            || (!joined_gt.is_empty() && glob_star_ordered_match(&joined_colon, pattern))
            || segments
                .iter()
                .any(|seg| glob_star_ordered_match(seg, pattern))
    } else {
        let pl = pattern.to_lowercase();
        segments
            .iter()
            .any(|name| name.eq_ignore_ascii_case(pattern) || name.to_lowercase().contains(&pl))
    }
}

/// `*` wildcard: non-empty fragments must appear left-to-right in `haystack` (case insensitive).
fn glob_star_ordered_match(haystack: &str, pattern: &str) -> bool {
    let hay = haystack.to_lowercase();
    let pat_lc = pattern.to_lowercase();
    let parts: Vec<&str> = pat_lc.split('*').filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return true;
    }
    let mut cursor = 0usize;
    for part in parts {
        match hay[cursor..].find(part) {
            Some(ix) => cursor += ix + part.len(),
            None => return false,
        }
    }
    true
}

fn split_by_keyword<'a>(input: &'a str, keyword: &str) -> Vec<&'a str> {
    let kw = format!(" {} ", keyword).to_lowercase();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut quote: Option<char> = None;
    let mut paren_depth = 0usize;
    let mut idx = 0usize;

    while idx < input.len() {
        let ch = input.as_bytes()[idx] as char;
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            idx += 1;
            continue;
        }
        if ch == '"' || ch == '\'' {
            quote = Some(ch);
            idx += 1;
            continue;
        }
        if ch == '(' {
            paren_depth += 1;
            idx += 1;
            continue;
        }
        if ch == ')' {
            paren_depth = paren_depth.saturating_sub(1);
            idx += 1;
            continue;
        }

        if paren_depth == 0 && input[idx..].to_lowercase().starts_with(&kw) {
            out.push(input[start..idx].trim());
            idx += kw.len();
            start = idx;
            continue;
        }
        idx += 1;
    }
    out.push(input[start..].trim());
    out.into_iter().filter(|s| !s.is_empty()).collect()
}

fn extract_global_not_project(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let lower = input.to_lowercase();
    let needle = "not project";
    let mut start = 0usize;

    while let Some(pos) = lower[start..].find(needle) {
        let idx = start + pos;
        let rest = input[idx + needle.len()..].trim_start();
        if let Some(eq_pos) = rest.find('=') {
            let value = strip_quotes(
                rest[eq_pos + 1..]
                    .trim()
                    .split_whitespace()
                    .next()
                    .unwrap_or(""),
            );
            if !value.is_empty() {
                out.push(value.to_string());
            }
        }
        start = idx + needle.len();
    }

    out
}

#[cfg(test)]
mod tests {
    use super::evaluate_query;
    use super::Query;
    use crate::models::action::Action;
    use std::collections::HashMap;

    #[test]
    fn query_matches_text_and_tags() {
        let q = Query::parse("trash @home").expect("query should parse");
        let action = Action {
            text: "Take out trash".to_string(),
            line_index: 0,
            project: Some("House".to_string()),
            project_chain: vec!["House".to_string()],
            notes: Vec::new(),
            tags: vec!["@home".to_string()],
            tag_values: HashMap::new(),
            done: false,
            due: None,
            source_file: "x.taskpaper".to_string(),
        };
        assert!(q.matches(&action));
    }

    #[test]
    fn simple_query_with_done_tag_enables_done_inclusion() {
        let q = Query::parse("@done").expect("query should parse");
        let action = Action {
            text: "Done task".to_string(),
            line_index: 0,
            project: Some("Work".to_string()),
            project_chain: vec!["Work".to_string()],
            notes: Vec::new(),
            tags: vec!["@done".to_string()],
            tag_values: HashMap::new(),
            done: true,
            due: None,
            source_file: "x.taskpaper".to_string(),
        };
        assert!(q.matches(&action));
    }

    #[test]
    fn next_defaults_require_na() {
        let q = Query::next_defaults("na");
        let a = Action {
            text: "Something @na".to_string(),
            line_index: 0,
            project: Some("Work".to_string()),
            project_chain: vec!["Work".to_string()],
            notes: Vec::new(),
            tags: vec!["@na".to_string()],
            tag_values: HashMap::new(),
            done: false,
            due: None,
            source_file: "x.taskpaper".to_string(),
        };
        assert!(q.matches(&a));
    }

    #[test]
    fn taskpaper_search_supports_or_and_not_project() {
        let q = Query::parse(
            r#"@search(@home or (project = "Errands" and not @done) and not project = "Archive")"#,
        )
        .expect("query should parse");
        let a = Action {
            text: "Buy milk".to_string(),
            line_index: 0,
            project: Some("Errands".to_string()),
            project_chain: vec!["Errands".to_string()],
            notes: Vec::new(),
            tags: vec!["@home".to_string()],
            tag_values: HashMap::new(),
            done: false,
            due: None,
            source_file: "x.taskpaper".to_string(),
        };
        let b = Action {
            project: Some("Archive".to_string()),
            project_chain: vec!["Archive".to_string()],
            ..a.clone()
        };
        assert!(q.matches(&a));
        assert!(!q.matches(&b));
    }

    #[test]
    fn taskpaper_search_supports_tag_comparisons() {
        let q = Query::parse(r#"@search(@priority > 3 and @context contains "home")"#)
            .expect("query should parse");
        let mut tag_values = HashMap::new();
        tag_values.insert("priority".to_string(), "5".to_string());
        tag_values.insert("context".to_string(), "home-office".to_string());
        let a = Action {
            text: "Deep work".to_string(),
            line_index: 0,
            project: Some("Work".to_string()),
            project_chain: vec!["Work".to_string()],
            notes: Vec::new(),
            tags: vec!["@priority".to_string(), "@context".to_string()],
            tag_values,
            done: false,
            due: None,
            source_file: "x.taskpaper".to_string(),
        };
        assert!(q.matches(&a));
    }

    #[test]
    fn taskpaper_search_supports_beginswith_operator() {
        let q = Query::parse(r#"@search(@context beginswith "home")"#).expect("query should parse");
        let mut tag_values = HashMap::new();
        tag_values.insert("context".to_string(), "home-office".to_string());
        let a = Action {
            text: "Deep work".to_string(),
            line_index: 0,
            project: Some("Work".to_string()),
            project_chain: vec!["Work".to_string()],
            notes: Vec::new(),
            tags: vec!["@context".to_string()],
            tag_values,
            done: false,
            due: None,
            source_file: "x.taskpaper".to_string(),
        };
        assert!(q.matches(&a));
    }

    #[test]
    fn taskpaper_search_supports_endswith_operator() {
        let q = Query::parse(r#"@search(@context endswith "office")"#).expect("query should parse");
        let mut tag_values = HashMap::new();
        tag_values.insert("context".to_string(), "home-office".to_string());
        let a = Action {
            text: "Deep work".to_string(),
            line_index: 0,
            project: Some("Work".to_string()),
            project_chain: vec!["Work".to_string()],
            notes: Vec::new(),
            tags: vec!["@context".to_string()],
            tag_values,
            done: false,
            due: None,
            source_file: "x.taskpaper".to_string(),
        };
        assert!(q.matches(&a));
    }

    #[test]
    fn taskpaper_search_supports_not_equal_operator() {
        let q = Query::parse(r#"@search(@priority != 1)"#).expect("query should parse");
        let mut tag_values = HashMap::new();
        tag_values.insert("priority".to_string(), "5".to_string());
        let a = Action {
            text: "Deep work".to_string(),
            line_index: 0,
            project: Some("Work".to_string()),
            project_chain: vec!["Work".to_string()],
            notes: Vec::new(),
            tags: vec!["@priority".to_string()],
            tag_values,
            done: false,
            due: None,
            source_file: "x.taskpaper".to_string(),
        };
        assert!(q.matches(&a));
    }

    #[test]
    fn taskpaper_search_not_equal_fails_for_same_value() {
        let q = Query::parse(r#"@search(@priority != 5)"#).expect("query should parse");
        let mut tag_values = HashMap::new();
        tag_values.insert("priority".to_string(), "5".to_string());
        let a = Action {
            text: "Deep work".to_string(),
            line_index: 0,
            project: Some("Work".to_string()),
            project_chain: vec!["Work".to_string()],
            notes: Vec::new(),
            tags: vec!["@priority".to_string()],
            tag_values,
            done: false,
            due: None,
            source_file: "x.taskpaper".to_string(),
        };
        assert!(!q.matches(&a));
    }

    #[test]
    fn taskpaper_search_supports_regex_match_operator() {
        let q = Query::parse(r#"@search(@context =~ "^home-.*$")"#).expect("query should parse");
        let mut tag_values = HashMap::new();
        tag_values.insert("context".to_string(), "home-office".to_string());
        let a = Action {
            text: "Deep work".to_string(),
            line_index: 0,
            project: Some("Work".to_string()),
            project_chain: vec!["Work".to_string()],
            notes: Vec::new(),
            tags: vec!["@context".to_string()],
            tag_values,
            done: false,
            due: None,
            source_file: "x.taskpaper".to_string(),
        };
        assert!(q.matches(&a));
    }

    #[test]
    fn taskpaper_search_regex_match_is_case_sensitive() {
        let q = Query::parse(r#"@search(@context =~ "^HOME-.*$")"#).expect("query should parse");
        let mut tag_values = HashMap::new();
        tag_values.insert("context".to_string(), "home-office".to_string());
        let a = Action {
            text: "Deep work".to_string(),
            line_index: 0,
            project: Some("Work".to_string()),
            project_chain: vec!["Work".to_string()],
            notes: Vec::new(),
            tags: vec!["@context".to_string()],
            tag_values,
            done: false,
            due: None,
            source_file: "x.taskpaper".to_string(),
        };
        assert!(!q.matches(&a));
    }

    #[test]
    fn taskpaper_search_supports_project_shortcut_without_equals() {
        let q =
            Query::parse(r#"@search(project Errands and not @done)"#).expect("query should parse");
        let a = Action {
            text: "Buy milk".to_string(),
            line_index: 0,
            project: Some("Errands".to_string()),
            project_chain: vec!["Errands".to_string()],
            notes: Vec::new(),
            tags: vec!["@na".to_string()],
            tag_values: HashMap::new(),
            done: false,
            due: None,
            source_file: "x.taskpaper".to_string(),
        };
        assert!(q.matches(&a));
    }

    #[test]
    fn taskpaper_search_matches_project_predicate_with_star_wildcard() {
        let q =
            Query::parse(r#"@search(project Work*Trail and @home)"#).expect("query should parse");
        let a = Action {
            text: "Trail item @home".to_string(),
            line_index: 0,
            project: Some("ClientTrail".to_string()),
            project_chain: vec!["Work".to_string(), "ClientTrail".to_string()],
            notes: Vec::new(),
            tags: vec!["@home".to_string()],
            tag_values: HashMap::new(),
            done: false,
            due: None,
            source_file: "x.taskpaper".to_string(),
        };
        assert!(q.matches(&a));
        let mismatch = Action {
            project_chain: vec!["Errands".to_string()],
            ..a.clone()
        };
        assert!(!q.matches(&mismatch));
    }

    #[test]
    fn taskpaper_search_supports_matches_operator() {
        let q =
            Query::parse(r#"@search(@context matches "^home-.*$")"#).expect("query should parse");
        let mut tag_values = HashMap::new();
        tag_values.insert("context".to_string(), "home-office".to_string());
        let a = Action {
            text: "Deep work".to_string(),
            line_index: 0,
            project: Some("Work".to_string()),
            project_chain: vec!["Work".to_string()],
            notes: Vec::new(),
            tags: vec!["@context".to_string()],
            tag_values,
            done: false,
            due: None,
            source_file: "x.taskpaper".to_string(),
        };
        assert!(q.matches(&a));
    }

    #[test]
    fn taskpaper_search_supports_case_modifier_i() {
        let q =
            Query::parse(r#"@search(@context contains[i] "HOME")"#).expect("query should parse");
        let mut tag_values = HashMap::new();
        tag_values.insert("context".to_string(), "home-office".to_string());
        let a = Action {
            text: "Deep work".to_string(),
            line_index: 0,
            project: Some("Work".to_string()),
            project_chain: vec!["Work".to_string()],
            notes: Vec::new(),
            tags: vec!["@context".to_string()],
            tag_values,
            done: false,
            due: None,
            source_file: "x.taskpaper".to_string(),
        };
        assert!(q.matches(&a));
    }

    #[test]
    fn taskpaper_search_respects_parenthesized_and_or_groups() {
        let q = Query::parse(r#"@search((@home or @office) and not @done)"#)
            .expect("query should parse");
        let home = Action {
            text: "Home task".to_string(),
            line_index: 0,
            project: Some("Inbox".to_string()),
            project_chain: vec!["Inbox".to_string()],
            notes: Vec::new(),
            tags: vec!["@home".to_string()],
            tag_values: HashMap::new(),
            done: false,
            due: None,
            source_file: "x.taskpaper".to_string(),
        };
        let office_done = Action {
            tags: vec!["@office".to_string(), "@done".to_string()],
            done: true,
            ..home.clone()
        };
        assert!(q.matches(&home));
        assert!(!q.matches(&office_done));
    }

    #[test]
    fn taskpaper_search_supports_wildcard_terms() {
        let q = Query::parse(r#"@search(road*map)"#).expect("query should parse");
        let a = Action {
            text: "Road to final map".to_string(),
            line_index: 0,
            project: Some("Work".to_string()),
            project_chain: vec!["Work".to_string()],
            notes: Vec::new(),
            tags: vec!["@na".to_string()],
            tag_values: HashMap::new(),
            done: false,
            due: None,
            source_file: "x.taskpaper".to_string(),
        };
        assert!(q.matches(&a));
    }

    #[test]
    fn taskpaper_search_matches_slash_item_path() {
        let q = Query::parse(r#"@search(/Work/ClientA)"#).expect("parse");
        let ok = Action {
            text: "t".to_string(),
            line_index: 0,
            project: Some("ClientA".to_string()),
            project_chain: vec!["Work".to_string(), "ClientA".to_string()],
            notes: Vec::new(),
            tags: Vec::new(),
            tag_values: HashMap::new(),
            done: false,
            due: None,
            source_file: "x.taskpaper".to_string(),
        };
        assert!(q.matches(&ok));
        let deeper = Action {
            project_chain: vec!["Work".to_string(), "ClientA".to_string(), "Ops".to_string()],
            project: Some("Ops".to_string()),
            ..ok.clone()
        };
        assert!(q.matches(&deeper));
        let wrong = Action {
            project_chain: vec!["Work".to_string(), "ClientB".to_string()],
            project: Some("ClientB".to_string()),
            ..ok.clone()
        };
        assert!(!q.matches(&wrong));
    }

    #[test]
    fn taskpaper_search_matches_descendant_item_path_double_slash() {
        let q = Query::parse(r#"@search(//Ops)"#).expect("parse");
        let a = Action {
            text: "t".to_string(),
            line_index: 0,
            project: Some("Ops".to_string()),
            project_chain: vec!["Work".to_string(), "ClientA".to_string(), "Ops".to_string()],
            notes: Vec::new(),
            tags: Vec::new(),
            tag_values: HashMap::new(),
            done: false,
            due: None,
            source_file: "x.taskpaper".to_string(),
        };
        assert!(q.matches(&a));
    }

    #[test]
    fn taskpaper_search_item_path_keyword_and_exclude() {
        let q = Query::parse(r#"@search(path /Work/* and not path /Work/Archive)"#).expect("parse");
        let active = Action {
            text: "t".to_string(),
            line_index: 0,
            project: Some("Team".to_string()),
            project_chain: vec!["Work".to_string(), "Team".to_string()],
            notes: Vec::new(),
            tags: Vec::new(),
            tag_values: HashMap::new(),
            done: false,
            due: None,
            source_file: "x.taskpaper".to_string(),
        };
        assert!(q.matches(&active));
        let archived = Action {
            project_chain: vec!["Work".to_string(), "Archive".to_string()],
            project: Some("Archive".to_string()),
            ..active.clone()
        };
        assert!(!q.matches(&archived));
    }

    #[test]
    fn taskpaper_search_trailing_slice_index_returns_first_match() {
        use crate::models::todo::TodoFile;
        use std::fs;

        let path = std::env::temp_dir().join(format!(
            "na_search_slice_idx_{}.taskpaper",
            std::process::id()
        ));
        let content = "Inbox:\n\t- First @na\n\t- Second @na\n\t- Third @na @done(2025-01-01)\n";
        fs::write(&path, content).expect("write temp taskpaper");
        let file = TodoFile::load(&path).expect("load");
        let q =
            Query::parse(r#"@search((project Inbox and @na and not @done)[0])"#).expect("parse");
        let actions = evaluate_query(&[file], &q);
        fs::remove_file(&path).ok();
        assert_eq!(actions.len(), 1);
        assert!(actions[0].text.contains("First"), "{:?}", actions[0].text);
    }

    #[test]
    fn taskpaper_search_trailing_slice_range_is_exclusive_end() {
        use crate::models::todo::TodoFile;
        use std::fs;

        let path = std::env::temp_dir().join(format!(
            "na_search_slice_range_{}.taskpaper",
            std::process::id()
        ));
        let content = "Inbox:\n\t- First @na\n\t- Second @na\n\t- Third @na @done(2025-01-01)\n";
        fs::write(&path, content).expect("write");
        let file = TodoFile::load(&path).expect("load");
        let q =
            Query::parse(r#"@search((project Inbox and @na and not @done)[0:2])"#).expect("parse");
        let actions = evaluate_query(&[file], &q);
        fs::remove_file(&path).ok();
        assert_eq!(actions.len(), 2);
        assert!(actions[0].text.contains("First"));
        assert!(actions[1].text.contains("Second"));
    }

    #[test]
    fn taskpaper_search_slice_negative_index_yields_no_results() {
        use crate::models::todo::TodoFile;
        use std::fs;

        let path = std::env::temp_dir().join(format!(
            "na_search_slice_neg_{}.taskpaper",
            std::process::id()
        ));
        let content = "Inbox:\n\t- First @na\n";
        fs::write(&path, content).expect("write");
        let file = TodoFile::load(&path).expect("load");
        let q = Query::parse(r#"@search((@na)[-1])"#).expect("parse");
        let actions = evaluate_query(&[file], &q);
        fs::remove_file(&path).ok();
        assert!(actions.is_empty());
    }
}
