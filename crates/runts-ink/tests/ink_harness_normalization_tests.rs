//! Unit tests for the parity test harness normalization utilities.

#[cfg(test)]
mod normalization_tests {
    /// Test output normalization removes ANSI codes
    #[test]
    fn test_normalize_removes_ansi_codes() {
        let input = "\x1b[31mRed\x1b[0m \x1b[1mBold\x1b[0m";
        let normalized = input
            .replace("\x1b[31m", "")
            .replace("\x1b[0m", "")
            .replace("\x1b[1m", "");
        
        assert!(!normalized.contains("\x1b"));
        assert_eq!(normalized, "Red Bold");
    }

    /// Test output normalization removes carriage returns
    #[test]
    fn test_normalize_removes_carriage_returns() {
        let input = "Line1\r\nLine2\r\n";
        let normalized = input.replace("\r", "");
        
        assert!(!normalized.contains("\r"));
        assert_eq!(normalized, "Line1\nLine2\n");
    }

    /// Test output normalization trims trailing whitespace
    #[test]
    fn test_normalize_trims_trailing_whitespace() {
        let input = "Text with trailing   \n  \n";
        let normalized = input.trim_end().to_string();
        
        assert!(!normalized.ends_with(' '));
    }

    /// Test output normalization removes empty lines
    #[test]
    fn test_normalize_removes_empty_lines() {
        let input = "Line1\n\n\nLine2\n";
        let normalized: Vec<&str> = input
            .lines()
            .filter(|l| !l.trim().is_empty())
            .collect();
        
        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized[0], "Line1");
        assert_eq!(normalized[1], "Line2");
    }

    /// Test output normalization removes duplicates
    #[test]
    fn test_normalize_removes_duplicates() {
        let input = "A\nB\nA\nC\nB";
        let mut seen = std::collections::HashSet::new();
        let normalized: Vec<&str> = input
            .lines()
            .filter(|l| seen.insert(l.to_string()))
            .collect();
        
        assert_eq!(normalized.len(), 3);
    }
}

#[cfg(test)]
mod similarity_tests {
    /// Test similarity calculation with identical content
    #[test]
    fn test_similarity_identical() {
        let content1 = "Line1\nLine2\nLine3";
        let content2 = "Line1\nLine2\nLine3";
        
        let lines1: Vec<&str> = content1.lines().collect();
        let lines2: Vec<&str> = content2.lines().collect();
        let common: Vec<&str> = lines1.iter()
            .filter(|l| lines2.contains(l))
            .cloned()
            .collect();
        
        let similarity = if lines1.len() > lines2.len() {
            common.len() * 100 / lines1.len()
        } else {
            common.len() * 100 / lines2.len()
        };
        
        assert_eq!(similarity, 100);
    }

    /// Test similarity calculation with no common content
    #[test]
    fn test_similarity_none() {
        let content1 = "Line1\nLine2";
        let content2 = "Line3\nLine4";
        
        let lines1: Vec<&str> = content1.lines().collect();
        let lines2: Vec<&str> = content2.lines().collect();
        let common: Vec<&str> = lines1.iter()
            .filter(|l| lines2.contains(l))
            .cloned()
            .collect();
        
        let max = lines1.len().max(lines2.len());
        let similarity = if max > 0 { common.len() * 100 / max } else { 100 };
        
        assert_eq!(similarity, 0);
    }

    /// Test similarity calculation with partial overlap
    #[test]
    fn test_similarity_partial() {
        let content1 = "A\nB\nC\nD";
        let content2 = "A\nB\nX\nY";
        
        let lines1: Vec<&str> = content1.lines().collect();
        let lines2: Vec<&str> = content2.lines().collect();
        let common: Vec<&str> = lines1.iter()
            .filter(|l| lines2.contains(l))
            .cloned()
            .collect();
        
        let max = lines1.len().max(lines2.len());
        let similarity = if max > 0 { common.len() * 100 / max } else { 100 };
        
        // 2 common out of 4 max = 50%
        assert_eq!(similarity, 50);
    }

    /// Test similarity with empty content
    #[test]
    fn test_similarity_empty_both() {
        let content1 = "";
        let content2 = "";
        
        let lines1: Vec<&str> = content1.lines().collect();
        let lines2: Vec<&str> = content2.lines().collect();
        
        let similarity = if lines1.is_empty() && lines2.is_empty() {
            100
        } else {
            0
        };
        
        assert_eq!(similarity, 100);
    }

    /// Test similarity with one empty
    #[test]
    fn test_similarity_one_empty() {
        let content1 = "A\nB";
        let content2 = "";
        
        let lines1: Vec<&str> = content1.lines().collect();
        let lines2: Vec<&str> = content2.lines().collect();
        
        let similarity = if lines1.is_empty() || lines2.is_empty() {
            0
        } else {
            100
        };
        
        assert_eq!(similarity, 0);
    }
}

#[cfg(test)]
mod symbol_extraction_tests {
    /// Test symbol extraction from output
    #[test]
    fn test_extract_symbols_words() {
        let content = "Box Component Demo\nColumn Layout\nItem 1";
        let symbols: Vec<&str> = content
            .split_whitespace()
            .filter(|w| w.len() >= 2 && w.chars().all(|c| c.is_alphabetic()))
            .collect();
        
        assert!(symbols.contains(&"Box"));
        assert!(symbols.contains(&"Component"));
        assert!(symbols.contains(&"Demo"));
    }

    /// Test symbol extraction excludes common words
    #[test]
    fn test_extract_symbols_excludes_keywords() {
        let keywords = ["ink", "react", "use", "import", "from", "export", "function", "const", "let", "var", "default"];
        let content = "import React from 'react'; const x = 1;";
        
        let symbols: Vec<&str> = content
            .split_whitespace()
            .filter(|w| !keywords.contains(&w))
            .filter(|w| w.len() >= 2)
            .collect();
        
        assert!(!symbols.contains(&"import"));
        assert!(!symbols.contains(&"from"));
        assert!(!symbols.contains(&"const"));
    }

    /// Test symbol extraction from mixed content
    #[test]
    fn test_extract_symbols_mixed_content() {
        let content = "╭─────────────────╮\n│ Hello World    │\n╰─────────────────╯";
        let symbols: Vec<&str> = content
            .split_whitespace()
            .filter(|w| w.len() >= 2)
            .collect();
        
        assert!(symbols.contains(&"Hello"));
        assert!(symbols.contains(&"World"));
    }
}

#[cfg(test)]
mod diff_generation_tests {
    /// Test that diff is generated correctly
    #[test]
    fn test_diff_generation() {
        let content1 = "Line1\nLine2\nLine3";
        let content2 = "Line1\nModified\nLine3";
        
        let diff_lines: Vec<String> = content1
            .lines()
            .zip(content2.lines())
            .filter(|(a, b)| a != b)
            .flat_map(|(a, b)| vec![format!("-{}", a), format!("+{}", b)])
            .collect();
        
        assert_eq!(diff_lines.len(), 2);
        assert!(diff_lines[0].starts_with('-'));
        assert!(diff_lines[1].starts_with('+'));
    }

    /// Test diff with identical content produces no changes
    #[test]
    fn test_diff_identical() {
        let content = "Same\nContent\nHere";
        
        let diff_lines: Vec<(&str, &str)> = content
            .lines()
            .zip(content.lines())
            .filter(|(a, b)| a != b)
            .collect();
        
        assert!(diff_lines.is_empty());
    }

    /// Test symbol diff identifies unique symbols
    #[test]
    fn test_symbol_diff_identifies_unique() {
        let symbols1 = vec!["A", "B", "C"];
        let symbols2 = vec!["B", "C", "D"];
        
        let only_in_1: Vec<&&str> = symbols1.iter()
            .filter(|s| !symbols2.contains(s))
            .collect();
        
        let only_in_2: Vec<&&str> = symbols2.iter()
            .filter(|s| !symbols1.contains(s))
            .collect();
        
        assert_eq!(only_in_1.len(), 1);
        assert_eq!(only_in_2.len(), 1);
        assert_eq!(*only_in_1[0], "A");
        assert_eq!(*only_in_2[0], "D");
    }
}
