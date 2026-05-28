//! Error handling utilities

use std::cmp::min;

pub fn levenshtein_distance(s1: &str, s2: &str) -> usize {
    let (s1, s2) = (s1.chars().collect::<Vec<_>>(), s2.chars().collect::<Vec<_>>());
    let (len1, len2) = (s1.len(), s2.len());
    if len1 == 0 { return len2; }
    if len2 == 0 { return len1; }
    let mut prev = (0..=len1).collect::<Vec<_>>();
    let mut curr = vec![0; len1 + 1];
    for i in 1..=len2 {
        curr[0] = i;
        for j in 1..=len1 {
            let cost = if s1[j - 1] == s2[i - 1] { 0 } else { 1 };
            curr[j] = min(min(prev[j] + 1, curr[j - 1] + 1), prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[len1]
}

pub fn suggest_correction(word: &str, valid: &[&str], max_distance: usize) -> Option<String> {
    valid.iter().map(|v| (v, levenshtein_distance(word, v))).filter(|(_, d)| *d <= max_distance && *d > 0).min_by_key(|(_, d)| *d).map(|(v, _)| (*v).to_string())
}

pub fn find_closest(word: &str, valid: &[&str]) -> Option<(String, usize)> {
    valid.iter().map(|v| (v, levenshtein_distance(word, v))).min_by_key(|(_, d)| *d).map(|(v, d)| ((*v).to_string(), d))
}

#[derive(Debug, Clone)]
pub struct LocatedError {
    pub message: String,
    pub file: Option<String>,
    pub line: usize,
    pub column: usize,
    pub source_line: Option<String>,
}

impl std::fmt::Display for LocatedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let (Some(file), Some(line)) = (&self.file, self.line.checked_add(1)) { write!(f, "{}:{}: ", file, line)?; }
        write!(f, "{}", self.message)?;
        if let Some(source) = &self.source_line { writeln!(f)?; write!(f, "    │ {}", source)?; }
        Ok(())
    }
}
