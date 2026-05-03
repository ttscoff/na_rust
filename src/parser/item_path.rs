//! TaskPaper-style slash paths (`/A/B`, `//Descendant`, `*` steps) shared by add/move resolution and `@search`.

use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathAxis {
    Child,
    Desc,
}

#[derive(Debug, Clone)]
pub(crate) struct PathStep {
    pub(crate) axis: PathAxis,
    pub(crate) text: String,
    pub(crate) wildcard: bool,
}

/// Parse a leading-slash item path (`/Foo//Bar`), used for project targeting and `@search`.
pub fn parse_item_path(path: &str) -> Vec<PathStep> {
    let s = path.trim();
    if !s.starts_with('/') {
        return Vec::new();
    }
    let bytes = s.as_bytes();
    let mut i = 0usize;
    let mut out = Vec::new();
    while i < bytes.len() {
        if bytes[i] != b'/' {
            break;
        }
        let axis = if i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            i += 2;
            PathAxis::Desc
        } else {
            i += 1;
            PathAxis::Child
        };
        let start = i;
        while i < bytes.len() && bytes[i] != b'/' {
            i += 1;
        }
        let text = s[start..i].trim().to_string();
        if text.is_empty() {
            continue;
        }
        out.push(PathStep {
            axis,
            wildcard: text == "*",
            text,
        });
    }
    out
}

/// Resolve path expression against flattened `project:chain:segments` listings (add/update/move).
pub fn resolve_item_path(path: &str, projects: &[String]) -> Vec<String> {
    let steps = parse_item_path(path);
    if steps.is_empty() {
        return Vec::new();
    }
    let chains: Vec<Vec<String>> = projects
        .iter()
        .map(|p| {
            p.split(':')
                .map(|s| s.trim().to_string())
                .collect::<Vec<_>>()
        })
        .collect();
    let mut current: Vec<Vec<String>> = vec![Vec::new()];
    for step in steps {
        let mut next = Vec::<Vec<String>>::new();
        match step.axis {
            PathAxis::Child => {
                for base in &current {
                    for chain in &chains {
                        if chain.len() <= base.len() {
                            continue;
                        }
                        if !chain.starts_with(base) {
                            continue;
                        }
                        let part = &chain[base.len()];
                        if step.wildcard || part.eq_ignore_ascii_case(&step.text) {
                            let candidate = chain[..=base.len()].to_vec();
                            if !next.iter().any(|n| n == &candidate) {
                                next.push(candidate);
                            }
                        }
                    }
                }
            }
            PathAxis::Desc => {
                for base in &current {
                    for chain in &chains {
                        if chain.len() <= base.len() || !chain.starts_with(base) {
                            continue;
                        }
                        for i in base.len()..chain.len() {
                            let part = &chain[i];
                            if step.wildcard || part.eq_ignore_ascii_case(&step.text) {
                                let candidate = chain[..=i].to_vec();
                                if !next.iter().any(|n| n == &candidate) {
                                    next.push(candidate);
                                }
                            }
                        }
                    }
                }
            }
        }
        current = next;
        if current.is_empty() {
            break;
        }
    }
    current.into_iter().map(|c| c.join(":")).collect()
}

/// True if `chain` denotes a project hierarchy that matches path expression (same steps as [`resolve_item_path`]).
pub fn project_chain_matches_path(chain: &[String], path: &str) -> bool {
    let steps = parse_item_path(path);
    if steps.is_empty() {
        return false;
    }
    let mut matched_prefix_lens: HashSet<usize> = HashSet::from([0]);
    for step in &steps {
        let mut next: HashSet<usize> = HashSet::new();
        for &prefix_len in &matched_prefix_lens {
            match step.axis {
                PathAxis::Child => {
                    if prefix_len < chain.len() {
                        let part = &chain[prefix_len];
                        if step.wildcard || part.eq_ignore_ascii_case(&step.text) {
                            next.insert(prefix_len + 1);
                        }
                    }
                }
                PathAxis::Desc => {
                    for j in prefix_len..chain.len() {
                        let part = &chain[j];
                        if step.wildcard || part.eq_ignore_ascii_case(&step.text) {
                            next.insert(j + 1);
                        }
                    }
                }
            }
        }
        matched_prefix_lens = next;
        if matched_prefix_lens.is_empty() {
            return false;
        }
    }
    matched_prefix_lens
        .iter()
        .any(|&len| len > 0 && len <= chain.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_child_and_descendant_segments() {
        let chain = vec!["Work".to_string(), "ClientA".to_string(), "Ops".to_string()];
        assert!(project_chain_matches_path(&chain, "/Work/ClientA"));
        assert!(project_chain_matches_path(&chain, "/Work/ClientA/Ops"));
        assert!(!project_chain_matches_path(&chain, "/Work/ClientB"));
        assert!(project_chain_matches_path(&chain, "//Ops"));
        assert!(project_chain_matches_path(&chain, "/Work/*/Ops"));
    }
}
