use crate::models::action::Action;
use crate::models::todo::TodoFile;
use anyhow::Result;

#[derive(Debug, Clone, Default)]
pub struct Query {
    clauses: Vec<Clause>,
    include_done: bool,
}

#[derive(Debug, Clone, Default)]
struct Clause {
    terms: Vec<String>,
    tags: Vec<String>,
    negated_tags: Vec<String>,
    comparisons: Vec<TagComparison>,
    project: Option<String>,
    exclude_projects: Vec<String>,
}

#[derive(Debug, Clone)]
struct TagComparison {
    tag: String,
    op: CompareOp,
    value: String,
    negated: bool,
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
        let include_done = clause
            .tags
            .iter()
            .any(|tag| tag.eq_ignore_ascii_case("@done") || tag.to_ascii_lowercase().starts_with("@done("));
        Ok(Self {
            clauses: vec![clause],
            include_done,
        })
    }

    pub fn next_defaults(na_tag: &str) -> Self {
        Self {
            clauses: vec![Clause {
                tags: vec![format!("@{}", na_tag.trim_start_matches('@'))],
                negated_tags: vec!["@done".to_string()],
                comparisons: Vec::new(),
                project: None,
                exclude_projects: vec!["Archive".to_string()],
                terms: Vec::new(),
            }],
            include_done: false,
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
        if let Some(stripped) = inner.strip_prefix("@search(").and_then(|s| s.strip_suffix(')')) {
            inner = stripped.trim();
        }

        let mut include_done = false;
        let global_excludes = extract_global_not_project(inner);
        let mut clauses = Vec::new();
        for raw_clause in split_by_keyword(inner, "or") {
            let mut clause = Clause::default();
            clause.exclude_projects.extend(global_excludes.clone());
            for part in split_by_keyword(raw_clause.trim(), "and") {
                let predicate = part.trim();
                if predicate.is_empty() {
                    continue;
                }
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
                    continue;
                }

                if p.starts_with('@') {
                    if let Some(comp) = parse_tag_comparison(p, negated) {
                        if comp.tag.eq_ignore_ascii_case("done") {
                            include_done = !negated;
                        }
                        clause.comparisons.push(comp);
                        continue;
                    }
                    if p.eq_ignore_ascii_case("@done") {
                        include_done = !negated;
                    }
                    if negated {
                        clause.negated_tags.push(p.to_string());
                    } else {
                        clause.tags.push(p.to_string());
                    }
                    continue;
                }

                clause.terms.push(strip_quotes(p).to_lowercase());
            }
            clauses.push(clause);
        }

        Ok(Self {
            clauses,
            include_done,
        })
    }

    fn matches_clause(clause: &Clause, action: &Action) -> bool {
        let action_lc = action.text.to_lowercase();
        let terms_ok = clause.terms.iter().all(|term| action_lc.contains(term));
        let tags_ok = clause.tags.iter().all(|tag| action.has_tag(tag));
        let negated_tags_ok = clause.negated_tags.iter().all(|tag| !action.has_tag(tag));
        let comparisons_ok = clause.comparisons.iter().all(|cmp| {
            let actual = action.tag_value(&cmp.tag);
            let base = compare_tag_value(actual, cmp);
            if cmp.negated { !base } else { base }
        });

        let project_names: Vec<&str> = if action.project_chain.is_empty() {
            action.project.as_deref().map(|p| vec![p]).unwrap_or_default()
        } else {
            action.project_chain.iter().map(String::as_str).collect()
        };
        let project_ok = clause.project.as_ref().is_none_or(|p| {
            project_names
                .iter()
                .any(|name| name.eq_ignore_ascii_case(p) || name.to_lowercase().contains(&p.to_lowercase()))
        });
        let excluded_ok = clause.exclude_projects.iter().all(|p| {
            project_names
                .iter()
                .all(|name| !name.eq_ignore_ascii_case(p) && !name.to_lowercase().contains(&p.to_lowercase()))
        });

        terms_ok && tags_ok && negated_tags_ok && comparisons_ok && project_ok && excluded_ok
    }

    pub fn matches(&self, action: &Action) -> bool {
        self.clauses.iter().any(|clause| Self::matches_clause(clause, action))
    }
}

pub fn evaluate_query(files: &[TodoFile], query: &Query) -> Vec<Action> {
    files
        .iter()
        .flat_map(TodoFile::actions)
        .filter(|a| (query.include_done || !a.done) && query.matches(a))
        .collect()
}

fn parse_project_predicate(input: &str) -> Option<String> {
    let lower = input.to_lowercase();
    if !lower.starts_with("project") {
        return None;
    }
    let parts = input.splitn(2, '=').collect::<Vec<_>>();
    if parts.len() != 2 {
        return None;
    }
    Some(strip_quotes(parts[1].trim()).to_string())
}

fn parse_tag_comparison(input: &str, negated: bool) -> Option<TagComparison> {
    let trimmed = input.trim();
    let no_at = trimmed.strip_prefix('@')?;
    let mut op = None;
    for candidate in [
        "beginswith",
        "endswith",
        "contains",
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
    let value = strip_quotes(no_at[pos + op_str.len()..].trim()).to_string();
    let op = match op_str {
        "=" | "==" | "!=" => CompareOp::Eq,
        ">" => CompareOp::Gt,
        "<" => CompareOp::Lt,
        ">=" => CompareOp::Ge,
        "<=" => CompareOp::Le,
        "=~" => CompareOp::Regex,
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
    })
}

fn compare_tag_value(actual: Option<&str>, cmp: &TagComparison) -> bool {
    let Some(actual) = actual else { return false };
    match cmp.op {
        CompareOp::Eq => actual.eq_ignore_ascii_case(&cmp.value),
        CompareOp::Contains => actual.to_lowercase().contains(&cmp.value.to_lowercase()),
        CompareOp::BeginsWith => actual
            .to_lowercase()
            .starts_with(&cmp.value.to_lowercase()),
        CompareOp::EndsWith => actual.to_lowercase().ends_with(&cmp.value.to_lowercase()),
        CompareOp::Regex => {
            regex::Regex::new(&cmp.value).map(|rx| rx.is_match(actual)).unwrap_or(false)
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

fn split_by_keyword<'a>(input: &'a str, keyword: &str) -> Vec<&'a str> {
    let kw = format!(" {} ", keyword).to_lowercase();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut quote: Option<char> = None;
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

        if input[idx..].to_lowercase().starts_with(&kw) {
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
            let value = strip_quotes(rest[eq_pos + 1..].trim().split_whitespace().next().unwrap_or(""));
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
    fn next_defaults_require_na_and_exclude_archive() {
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
        let b = Action {
            project: Some("Archive".to_string()),
            project_chain: vec!["Archive".to_string()],
            ..a.clone()
        };
        assert!(q.matches(&a));
        assert!(!q.matches(&b));
    }

    #[test]
    fn taskpaper_search_supports_or_and_not_project() {
        let q = Query::parse(r#"@search(@home or (project = "Errands" and not @done) and not project = "Archive")"#)
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
        let q =
            Query::parse(r#"@search(@context beginswith "home")"#).expect("query should parse");
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
}
