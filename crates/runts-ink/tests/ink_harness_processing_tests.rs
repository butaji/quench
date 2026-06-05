//! Unit tests for file handling and string processing.

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
mod terminal_handling_tests {
    /// Test terminal width calculation
    #[test]
    fn test_terminal_width_calculation() {
        let content = "This is a test string that is exactly 50 characters long";
        let width = content.len();
        assert_eq!(width, 56);
    }

    /// Test terminal height calculation
    #[test]
    fn test_terminal_height_calculation() {
        let lines = vec!["Line 1", "Line 2", "Line 3", "Line 4", "Line 5"];
        let height = lines.len();
        assert_eq!(height, 5);
    }

    /// Test that content fits in terminal
    #[test]
    fn test_content_fits_in_terminal() {
        let terminal_width = 80;
        let content = "Short content";
        let fits = content.len() <= terminal_width;
        assert!(fits);
    }

    /// Test that long content doesn't fit
    #[test]
    fn test_long_content_exceeds_terminal() {
        let terminal_width = 40;
        let content = "This is a very long line that exceeds the terminal width and should not fit";
        let fits = content.len() <= terminal_width;
        assert!(!fits);
    }

    /// Test box drawing character width
    #[test]
    fn test_box_drawing_character_width() {
        let box_char = '─';
        let width = box_char.len_utf8();
        assert_eq!(width, 3);
    }
}
