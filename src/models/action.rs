use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub text: String,
    pub line_index: usize,
    pub project: Option<String>,
    pub project_chain: Vec<String>,
    pub notes: Vec<String>,
    pub tags: Vec<String>,
    pub tag_values: HashMap<String, String>,
    pub done: bool,
    pub due: Option<NaiveDate>,
    pub source_file: String,
}

impl Action {
    pub fn has_tag(&self, needle: &str) -> bool {
        self.tags.iter().any(|tag| tag.eq_ignore_ascii_case(needle))
    }

    pub fn tag_value(&self, needle: &str) -> Option<&str> {
        let key = needle.trim_start_matches('@').to_ascii_lowercase();
        self.tag_values.get(&key).map(String::as_str)
    }
}

impl Display for Action {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some(project) = &self.project {
            write!(f, "{} :: {}", project, self.text)
        } else {
            write!(f, "{}", self.text)
        }
    }
}
