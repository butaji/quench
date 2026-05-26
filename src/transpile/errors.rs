//! Error handling and formatting utilities
//!
//! Provides:
//! - Levenshtein distance for "Did you mean..." suggestions
//! - Source context formatting
//! - User-friendly error messages

use std::collections::HashMap;

/// Calculate Levenshtein distance between two strings
pub fn levenshtein_distance(s1: &str, s2: &str) -> usize {
    let s1_chars: Vec<char> = s1.chars().collect();
    let s2_chars: Vec<char> = s2.chars().collect();
    
    let len1 = s1_chars.len();
    let len2 = s2_chars.len();
    
    if len1 == 0 {
        return len2;
    }
    if len2 == 0 {
        return len1;
    }
    
    // Use a 2-row matrix for memory efficiency
    let mut prev_row = (0..=len1).collect::<Vec<_>>();
    let mut curr_row = vec![0; len1 + 1];
    
    for i in 1..=len2 {
        curr_row[0] = i;
        
        for j in 1..=len1 {
            let cost = if s1_chars[j - 1] == s2_chars[i - 1] { 0 } else { 1 };
            curr_row[j] = std::cmp::min(
                std::cmp::min(
                    prev_row[j] + 1,      // deletion
                    curr_row[j - 1] + 1,  // insertion
                ),
                prev_row[j - 1] + cost,   // substitution
            );
        }
        
        std::mem::swap(&mut prev_row, &mut curr_row);
    }
    
    prev_row[len1]
}

/// Suggest a correction based on Levenshtein distance
pub fn suggest_correction(word: &str, valid: &[&str], max_distance: usize) -> Option<String> {
    valid
        .iter()
        .map(|v| (v, levenshtein_distance(word, v)))
        .filter(|(_, dist)| *dist <= max_distance && *dist > 0)
        .min_by_key(|(_, dist)| *dist)
        .map(|(v, _)| (*v).to_string())
}

/// Find the closest match in a list of valid options
pub fn find_closest(word: &str, valid: &[&str]) -> Option<(String, usize)> {
    valid
        .iter()
        .map(|v| (v, levenshtein_distance(word, v)))
        .min_by_key(|(_, dist)| *dist)
        .map(|(v, dist)| ((*v).to_string(), dist))
}

/// Error with source location
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
        if let (Some(file), Some(line)) = (&self.file, self.line.checked_add(1)) {
            write!(f, "{}:{}: ", file, line)?;
        }
        write!(f, "{}", self.message)?;
        
        if let Some(source) = &self.source_line {
            writeln!(f)?;
            write!(f, "    │ {}", source)?;
            if let Some(col) = self.column.checked_add(5) {
                writeln!(f)?;
                write!(f, "    │ ")?;
                for _ in 0..col {
                    write!(f, " ")?;
                }
                write!(f, "^")?;
            }
        }
        
        Ok(())
    }
}

/// Format an error with surrounding context
pub fn format_error_context(
    source: &str,
    line: usize,
    column: usize,
    message: &str,
    context_lines: usize,
) -> String {
    let lines: Vec<&str> = source.lines().collect();
    
    let start = if line >= context_lines {
        line - context_lines
    } else {
        0
    };
    
    let end = std::cmp::min(lines.len(), line + context_lines + 1);
    
    let mut output = String::new();
    
    for (i, line_text) in lines[start..end].iter().enumerate() {
        let line_num = start + i + 1;
        let marker = if line_num == line + 1 { ">>>" } else { "   " };
        
        output.push_str(&format!("{} │ {:4} │ {}\n", marker, line_num, line_text));
        
        if line_num == line + 1 {
            let col = column.min(line_text.len());
            let spaces = format!("    │ {:4} │ ", "");
            output.push_str(&spaces);
            for _ in 0..col {
                output.push(' ');
            }
            output.push_str("^\n");
        }
    }
    
    output.push_str(&format!("Error: {}\n", message));
    
    output
}

/// Common TypeScript/Rust mistyped keywords
pub static TS_KEYWORDS: [&str; 29] = [
    "function", "const", "let", "var", "if", "else", "for", "while", "do",
    "switch", "case", "break", "continue", "return", "try", "catch", "finally",
    "throw", "class", "extends", "import", "export", "default", "from", "as",
    "async", "await", "new", "typeof",
];

/// Common Preact hooks
pub static PREACT_HOOKS: [&str; 12] = [
    "useState", "useEffect", "useRef", "useMemo", "useCallback", "useReducer",
    "useContext", "useId", "useSignal", "useComputed", "useSignalEffect", "useLayoutEffect",
];

/// Common Fresh imports
pub static FRESH_IMPORTS: [&str; 7] = [
    "PageProps", "FreshContext", "Handler", "HandlerContext",
    "State", "Request", "Head",
];

/// Suggest a correction for a mistyped identifier
pub fn suggest_identifier(word: &str) -> Option<String> {
    // Check hooks first
    if let Some(suggestion) = suggest_correction(word, &PREACT_HOOKS, 3) {
        return Some(suggestion);
    }
    
    // Check Fresh imports
    if let Some(suggestion) = suggest_correction(word, &FRESH_IMPORTS, 3) {
        return Some(suggestion);
    }
    
    // Check TypeScript keywords
    suggest_correction(word, &TS_KEYWORDS, 2)
}

/// Error code prefix
#[derive(Debug)]
pub struct ErrorCode(pub &'static str);

impl ErrorCode {
    pub const E001: ErrorCode = ErrorCode("E001"); // Parse error
    pub const E002: ErrorCode = ErrorCode("E002"); // Type error
    pub const E003: ErrorCode = ErrorCode("E003"); // Unsupported feature
    pub const E004: ErrorCode = ErrorCode("E004"); // Island error
    pub const E005: ErrorCode = ErrorCode("E005"); // Handler error
    pub const E006: ErrorCode = ErrorCode("E006"); // Route error
    pub const E007: ErrorCode = ErrorCode("E007"); // Import error
    pub const E008: ErrorCode = ErrorCode("E008"); // Build error
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Formatted error with code and message
#[derive(Debug)]
pub struct FormattedError {
    pub code: ErrorCode,
    pub message: String,
    pub location: Option<String>,
    pub suggestion: Option<String>,
}

impl FormattedError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            location: None,
            suggestion: None,
        }
    }
    
    pub fn at(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }
    
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }
    
    pub fn suggest_identifier_word(word: &str) -> Option<String> {
        suggest_identifier(word)
    }
}

impl std::fmt::Display for FormattedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)?;
        
        if let Some(loc) = &self.location {
            write!(f, " at {}", loc)?;
        }
        
        if let Some(suggestion) = &self.suggestion {
            write!(f, "\n  Did you mean '{}'?", suggestion)?;
        }
        
        Ok(())
    }
}

impl std::error::Error for FormattedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

/// Build errors index for quick lookup
pub type ErrorIndex = HashMap<&'static str, Vec<&'static str>>;

/// Build common error patterns
pub fn build_error_index() -> ErrorIndex {
    let mut index = HashMap::new();
    
    index.insert("useState", vec!["use_state", "useState"]);
    index.insert("useEffect", vec!["use_effect", "useEffect"]);
    index.insert("useRef", vec!["use_ref", "useRef"]);
    index.insert("PageProps", vec!["PageProps", "PageProps"]);
    index.insert("FreshContext", vec!["FreshContext", "FreshContext"]);
    
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levenshtein_distance() {
        assert_eq!(levenshtein_distance("hello", "hello"), 0);
        assert_eq!(levenshtein_distance("hello", "hallo"), 1);
        assert_eq!(levenshtein_distance("hello", "world"), 4);
        assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
    }

    #[test]
    fn test_suggest_correction() {
        let valid = vec!["useState", "useEffect", "useRef"];
        
        assert_eq!(suggest_correction("useSatet", &valid, 2), Some("useState".to_string()));
        assert_eq!(suggest_correction("useEffct", &valid, 2), Some("useEffect".to_string()));
        assert_eq!(suggest_correction("completely_wrong", &valid, 10), None);
    }

    #[test]
    fn test_suggest_identifier() {
        assert_eq!(suggest_identifier("useSatet").unwrap(), "useState");
        assert_eq!(suggest_identifier("PageProps"), None); // Exact match
    }

    #[test]
    fn test_formatted_error() {
        let err = FormattedError::new(ErrorCode::E001, "Parse error")
            .at("/routes/index.tsx:10:5")
            .with_suggestion("useState");
        
        let output = err.to_string();
        assert!(output.contains("[E001]"));
        assert!(output.contains("Parse error"));
        assert!(output.contains("/routes/index.tsx:10:5"));
        assert!(output.contains("Did you mean 'useState'"));
    }
}
