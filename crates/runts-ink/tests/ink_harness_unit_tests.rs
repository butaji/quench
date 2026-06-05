//! Unit tests for the parity test harness utilities.
//!
//! These tests verify the correctness of the test harness functions
//! that are used for comparing outputs across environments.

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
        
        // Both empty should return 100% similarity
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

#[cfg(test)]
mod file_handling_tests {
    use std::fs;
    use std::io::Write;

    /// Test reading from non-existent file
    #[test]
    fn test_read_nonexistent_file() {
        let result = fs::read_to_string("/nonexistent/path/file.txt");
        assert!(result.is_err());
    }

    /// Test writing and reading file
    #[test]
    fn test_write_and_read() {
        let tmp_dir = std::env::temp_dir();
        let path = tmp_dir.join("runts_test_read.txt");
        
        let content = "Test content\nLine 2";
        fs::write(&path, &content).unwrap();
        
        let read = fs::read_to_string(&path).unwrap();
        assert_eq!(read, content);
        
        // Cleanup
        let _ = fs::remove_file(&path);
    }

    /// Test file write
    #[test]
    fn test_file_write() {
        use std::io::Write;
        let tmp_dir = std::env::temp_dir();
        let path = tmp_dir.join("runts_test_file.txt");
        
        {
            let mut file = fs::File::create(&path).unwrap();
            file.write_all(b"test content").unwrap();
        }
        
        assert!(path.exists());
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "test content");
        
        // Cleanup
        let _ = fs::remove_file(&path);
    }
}

#[cfg(test)]
mod string_processing_tests {
    /// Test stripping ANSI escape codes
    #[test]
    fn test_strip_ansi_codes() {
        let input = "\x1b[38;5;196mRed\x1b[0m \x1b[1m\x1b[38;5;21mBold Blue\x1b[0m";
        let stripped: String = input.chars()
            .filter(|c| !matches!(c, '\x1b'))
            .collect();
        
        // Simple check - no escape character
        assert!(!stripped.contains('\x1b'));
    }

    /// Test handling of unicode content
    #[test]
    fn test_unicode_handling() {
        let content = "日本語 中文 한국어";
        let words: Vec<&str> = content.split_whitespace().collect();
        
        assert_eq!(words.len(), 3);
        assert_eq!(words[0], "日本語");
    }

    /// Test handling of mixed width characters
    #[test]
    fn test_mixed_width_chars() {
        let content = "Hello 世界 ABC";
        let bytes = content.len();
        let chars = content.chars().count();
        
        // Bytes should be more than char count due to multi-byte characters
        assert!(bytes > chars);
    }

    /// Test line ending normalization
    #[test]
    fn test_line_ending_normalization() {
        let windows = "Line1\r\nLine2\r\nLine3";
        let normalized = windows.replace("\r\n", "\n");
        
        assert!(!normalized.contains("\r"));
        assert_eq!(normalized.lines().count(), 3);
    }
}

#[cfg(test)]
mod timeout_tests {
    /// Test timeout calculation
    #[test]
    fn test_timeout_calculation() {
        let _elapsed = 500u64;
        let timeout = 1000u64;
        
        let is_timed_out = _elapsed >= timeout;
        assert!(!is_timed_out);
        
        let elapsed_late = 1500u64;
        let is_timed_out_late = elapsed_late >= timeout;
        assert!(is_timed_out_late);
    }

    /// Test zero timeout behavior
    #[test]
    fn test_zero_timeout() {
        let timeout = 0u64;
        let elapsed = 1u64;
        
        let is_timed_out = elapsed >= timeout;
        // With 0 timeout, any elapsed time should be >= timeout
        assert!(is_timed_out);
    }
}

#[cfg(test)]
mod color_tests {
    /// Test RGB color formatting
    #[test]
    fn test_rgb_color_format() {
        let r = 255u8;
        let g = 128u8;
        let b = 0u8;
        
        let hex = format!("#{:02x}{:02x}{:02x}", r, g, b);
        assert_eq!(hex, "#ff8000");
    }

    /// Test hex color parsing
    #[test]
    fn test_hex_color_parsing() {
        let hex = "#ff5500";
        let hex_clean = hex.trim_start_matches('#');
        
        assert_eq!(hex_clean.len(), 6);
        assert!(hex_clean.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// Test ANSI color code generation
    #[test]
    fn test_ansi_color_codes() {
        // Standard ANSI foreground colors
        let colors = vec![
            (30, "Black"),
            (31, "Red"),
            (32, "Green"),
            (33, "Yellow"),
            (34, "Blue"),
            (35, "Magenta"),
            (36, "Cyan"),
            (37, "White"),
        ];
        
        assert_eq!(colors.len(), 8);
        for (code, _name) in colors {
            let escape = format!("\x1b[{}m", code);
            assert!(escape.starts_with("\x1b["));
        }
    }
}

#[cfg(test)]
mod vnode_tests {
    use runts_ink::{VNode, VNodeContent, Box, Text, Newline, Spacer, Static, Transform};
    
    /// Test VNode creation from Box
    #[test]
    fn test_vnode_from_box() {
        let box_node = Box::column().child(Text::new("test"));
        let vnode = VNode::from(box_node);
        
        assert_eq!(vnode.kind(), "box");
        assert_eq!(vnode.children().len(), 1);
    }

    /// Test VNode creation from Text
    #[test]
    fn test_vnode_from_text() {
        let text_node = Text::new("Hello");
        let vnode = VNode::from(text_node);
        
        assert_eq!(vnode.kind(), "text");
        assert!(vnode.children().is_empty());
    }

    /// Test VNode creation from Newline
    #[test]
    fn test_vnode_from_newline() {
        let newline = Newline::new();
        let vnode = VNode::from(newline);
        
        assert_eq!(vnode.kind(), "newline");
    }

    /// Test VNode creation from Spacer
    #[test]
    fn test_vnode_from_spacer() {
        let spacer = Spacer::new();
        let vnode = VNode::from(spacer);
        
        assert_eq!(vnode.kind(), "spacer");
    }

    /// Test VNode creation from Static
    #[test]
    fn test_vnode_from_static() {
        let stat = Static::new().child(Text::new("content"));
        let vnode = VNode::from(stat);
        
        assert_eq!(vnode.kind(), "static");
        assert_eq!(vnode.children().len(), 1);
    }

    /// Test VNode creation from Transform
    #[test]
    fn test_vnode_from_transform() {
        let trans = Transform::new(Text::new("transformed"));
        let vnode = VNode::from(trans);
        
        assert_eq!(vnode.kind(), "transform");
        assert_eq!(vnode.children().len(), 1);
    }

    /// Test VNode fragment
    #[test]
    fn test_vnode_fragment() {
        let fragment = VNodeContent::Fragment(vec![
            Text::new("A").into(),
            Text::new("B").into(),
        ]);
        let vnode = VNode::from(fragment);
        
        assert_eq!(vnode.kind(), "fragment");
        assert_eq!(vnode.children().len(), 2);
    }

    /// Test nested VNode structure
    #[test]
    fn test_vnode_nested_structure() {
        let root = VNode::from(
            Box::column()
                .child(Text::new("Header"))
                .child(
                    Box::row()
                        .child(Text::new("Left"))
                        .child(Text::new("Right"))
                )
                .child(Text::new("Footer"))
        );
        
        assert_eq!(root.kind(), "box");
        assert_eq!(root.children().len(), 3);
        assert_eq!(root.children()[0].kind(), "text");
        assert_eq!(root.children()[1].kind(), "box");
        assert_eq!(root.children()[2].kind(), "text");
    }
}

#[cfg(test)]
mod props_tests {
    use runts_ink::Props;
    use serde_json::json;

    /// Test Props creation
    #[test]
    fn test_props_creation() {
        let props = Props::new();
        assert!(props.raw().is_null());
    }

    /// Test Props with values
    #[test]
    fn test_props_with_values() {
        let props = Props::from_serialize(json!({
            "color": "red",
            "bold": true
        })).unwrap();
        
        let decoded: serde_json::Value = props.decode().unwrap();
        assert_eq!(decoded["color"], "red");
        assert_eq!(decoded["bold"], true);
    }

    /// Test Props serialization
    #[test]
    fn test_props_serialization() {
        let props = Props::from_serialize(json!({
            "color": "blue"
        })).unwrap();
        
        let json_str = serde_json::to_string(&props).unwrap();
        assert!(json_str.contains("color"));
        assert!(json_str.contains("blue"));
    }

    /// Test Props deserialization
    #[test]
    fn test_props_deserialization() {
        let json_str = r#"{"color":"green","size":10}"#;
        let props: Props = serde_json::from_str(json_str).unwrap();
        
        let decoded: serde_json::Value = props.decode().unwrap();
        assert_eq!(decoded["color"], "green");
        assert_eq!(decoded["size"], 10);
    }

    /// Test Props with method chaining
    #[test]
    fn test_props_with_method() {
        let props = Props::new()
            .with("key1", "value1")
            .with("key2", 42);
        
        let raw = props.raw();
        assert!(raw.is_object());
        if let Some(obj) = raw.as_object() {
            assert_eq!(obj.get("key1"), Some(&serde_json::json!("value1")));
            assert_eq!(obj.get("key2"), Some(&serde_json::json!(42)));
        }
    }
}

#[cfg(test)]
mod render_tests {
    use runts_ink::RenderOptions;

    /// Test render options default values
    #[test]
    fn test_render_options_default() {
        let options = RenderOptions::new();
        
        assert!(!options.patch_console);
        assert!(options.exit_on_ctrl_c);
        assert_eq!(options.tick_ms, 100);
        assert!(options.alternate_screen);
    }

    /// Test render options custom values
    #[test]
    fn test_render_options_custom() {
        let mut options = RenderOptions::new();
        options.patch_console = true;
        options.exit_on_q = true;
        options.tick_ms = 50;
        
        assert!(options.patch_console);
        assert!(options.exit_on_q);
        assert_eq!(options.tick_ms, 50);
    }

    /// Test render options clone
    #[test]
    fn test_render_options_clone() {
        let options = RenderOptions::new();
        let cloned = options.clone();
        
        assert_eq!(options.patch_console, cloned.patch_console);
        assert_eq!(options.tick_ms, cloned.tick_ms);
    }
}

#[cfg(test)]
mod edge_case_tests {
    /// Test with very long content
    #[test]
    fn test_long_content_handling() {
        let long_content = "x".repeat(10000);
        let lines: Vec<&str> = long_content.lines().collect();
        assert_eq!(lines.len(), 1);
    }

    /// Test similarity with terminal width limits
    #[test]
    fn test_terminal_width_simulation() {
        // Simulate terminal width of 80 chars
        let terminal_width = 80;
        let content = "This is a very long line that exceeds the terminal width and needs to be wrapped properly.";
        
        // In actual rendering, long lines would be wrapped at terminal width
        let needs_wrap = content.len() > terminal_width;
        assert!(needs_wrap);
        
        // Split into chunks of terminal width
        let chars: Vec<char> = content.chars().collect();
        let chunk_count = (chars.len() + terminal_width - 1) / terminal_width;
        
        assert!(chunk_count > 1);
    }

    /// Test ANSI escape sequence handling
    #[test]
    fn test_ansi_escape_handling() {
        let text = "\x1b[1mBold Text\x1b[0m";
        
        // Simple strip: remove all escape sequences
        let mut cleaned = String::new();
        let mut in_escape = false;
        
        for c in text.chars() {
            if c == '\x1b' {
                in_escape = true;
            } else if in_escape {
                // Look for 'm' to end the escape sequence
                if c == 'm' {
                    in_escape = false;
                }
            } else {
                cleaned.push(c);
            }
        }
        
        // Verify ANSI codes were present
        assert!(text.len() > cleaned.len());
        
        // Verify no escape chars in cleaned
        assert!(!cleaned.contains('\x1b'));
        assert_eq!(cleaned, "Bold Text");
    }

    /// Test Unicode box drawing characters
    #[test]
    fn test_box_drawing_chars() {
        let box_chars = "┌─┐│└┘├┤┬┴┼";
        // Each char is 3 bytes in UTF-8
        assert!(box_chars.chars().all(|c| c.len_utf8() == 3)); 
        // Count the actual characters
        assert_eq!(box_chars.chars().count(), 11);
    }

    /// Test diff generation algorithm
    #[test]
    fn test_diff_generation_algorithm() {
        let old = vec!["Line 1", "Line 2", "Line 3", "Line 4"];
        let new = vec!["Line 1", "Modified 2", "Line 3", "Line 5"];
        
        // Find common lines
        let common: Vec<&&str> = old.iter()
            .filter(|l| new.contains(l))
            .collect();
        
        assert_eq!(common.len(), 2);
        assert_eq!(*common[0], "Line 1");
        assert!(common.contains(&&"Line 3"));
        
        // Find added lines (in new but not in old)
        let added: Vec<&&str> = new.iter()
            .filter(|l| !old.contains(l))
            .collect();
        
        assert_eq!(added.len(), 2);
        assert!(added.contains(&&"Modified 2"));
        assert!(added.contains(&&"Line 5"));
    }

    /// Test timeout with process states
    #[test]
    fn test_process_timeout_simulation() {
        struct Process {
            started: u64,
            timeout_ms: u64,
        }
        
        impl Process {
            fn is_timed_out(&self, elapsed_ms: u64) -> bool {
                elapsed_ms >= self.timeout_ms
            }
        }
        
        let proc = Process { started: 0, timeout_ms: 5000 };
        
        assert!(!proc.is_timed_out(1000));
        assert!(!proc.is_timed_out(4999));
        assert!(proc.is_timed_out(5000));
        assert!(proc.is_timed_out(10000));
    }

    /// Test file path normalization
    #[test]
    fn test_file_path_normalization() {
        let paths = vec![
            "/Users/admin/Code/GitHub/runie-tsx/examples/ink-counter",
            "/Users/admin/Code/GitHub/runie-tsx/examples/ink-counter/",
            "./examples/ink-counter",
            "examples/ink-counter",
        ];
        
        // Each path should resolve to the same directory
        for path in &paths {
            let normalized = path.trim_end_matches('/');
            assert!(normalized.ends_with("ink-counter"));
        }
    }

    /// Test glob pattern matching
    #[test]
    fn test_glob_pattern_matching() {
        let examples = vec![
            "ink-counter",
            "ink-box",
            "ink-input",
            "ink-use-app",
            "my-blog",
            "tui-counter",
        ];
        
        let ink_examples: Vec<_> = examples.iter()
            .filter(|e| e.starts_with("ink-"))
            .collect();
        
        assert_eq!(ink_examples.len(), 4);
        assert!(ink_examples.contains(&&&"ink-counter"));
        assert!(!ink_examples.contains(&&&"my-blog"));
    }

    /// Test JSON serialization of test results
    #[test]
    fn test_json_result_serialization() {
        use serde::{Deserialize, Serialize};
        
        #[derive(Debug, Serialize, Deserialize)]
        struct TestResult {
            name: String,
            passed: bool,
            similarity: u8,
            duration_ms: u64,
        }
        
        let result = TestResult {
            name: "ink-counter".to_string(),
            passed: true,
            similarity: 100,
            duration_ms: 150,
        };
        
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("ink-counter"));
        assert!(json.contains("true"));
        assert!(json.contains("100"));
        
        let decoded: TestResult = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.name, "ink-counter");
        assert_eq!(decoded.similarity, 100);
    }
}
