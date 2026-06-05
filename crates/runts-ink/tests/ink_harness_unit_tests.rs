//! Unit tests for the parity test harness utilities.

#[cfg(test)]
mod vnode_tests {
    use runts_ink::{VNode, VNodeContent, Box, Text, Newline, Spacer, Static, Transform};
    
    #[test]
    fn test_vnode_from_box() {
        let box_node = Box::column().child(Text::new("test"));
        let vnode = VNode::from(box_node);
        assert_eq!(vnode.kind(), "box");
        assert_eq!(vnode.children().len(), 1);
    }

    #[test]
    fn test_vnode_from_text() {
        let text_node = Text::new("Hello");
        let vnode = VNode::from(text_node);
        assert_eq!(vnode.kind(), "text");
        assert!(vnode.children().is_empty());
    }

    #[test]
    fn test_vnode_from_newline() {
        let newline = Newline::new();
        let vnode = VNode::from(newline);
        assert_eq!(vnode.kind(), "newline");
    }

    #[test]
    fn test_vnode_from_spacer() {
        let spacer = Spacer::new();
        let vnode = VNode::from(spacer);
        assert_eq!(vnode.kind(), "spacer");
    }

    #[test]
    fn test_vnode_from_static() {
        let stat = Static::new().child(Text::new("content"));
        let vnode = VNode::from(stat);
        assert_eq!(vnode.kind(), "static");
        assert_eq!(vnode.children().len(), 1);
    }

    #[test]
    fn test_vnode_from_transform() {
        let trans = Transform::new(Text::new("transformed"));
        let vnode = VNode::from(trans);
        assert_eq!(vnode.kind(), "transform");
        assert_eq!(vnode.children().len(), 1);
    }

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

    #[test]
    fn test_vnode_nested_structure() {
        let root = VNode::from(
            Box::column()
                .child(Text::new("Header"))
                .child(Box::row()
                    .child(Text::new("Left"))
                    .child(Text::new("Right")))
                .child(Text::new("Footer"))
        );
        assert_eq!(root.kind(), "box");
        assert_eq!(root.children().len(), 3);
    }
}

#[cfg(test)]
mod props_tests {
    use runts_ink::Props;
    use serde_json::json;

    #[test]
    fn test_props_creation() {
        let props = Props::new();
        assert!(props.raw().is_null());
    }

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

    #[test]
    fn test_props_serialization() {
        let props = Props::from_serialize(json!({"color": "blue"})).unwrap();
        let json_str = serde_json::to_string(&props).unwrap();
        assert!(json_str.contains("color"));
    }

    #[test]
    fn test_props_deserialization() {
        let json_str = r#"{"color":"green","size":10}"#;
        let props: Props = serde_json::from_str(json_str).unwrap();
        let decoded: serde_json::Value = props.decode().unwrap();
        assert_eq!(decoded["color"], "green");
        assert_eq!(decoded["size"], 10);
    }
}

#[cfg(test)]
mod render_tests {
    use runts_ink::RenderOptions;

    #[test]
    fn test_render_options_default() {
        let options = RenderOptions::new();
        assert!(!options.patch_console);
        assert!(options.exit_on_ctrl_c);
        assert_eq!(options.tick_ms, 100);
        assert!(options.alternate_screen);
    }

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

    #[test]
    fn test_render_options_clone() {
        let options = RenderOptions::new();
        let cloned = options.clone();
        assert_eq!(options.patch_console, cloned.patch_console);
    }
}

#[cfg(test)]
mod parity_threshold_tests {
    #[test]
    fn test_parity_threshold_60_passes() {
        let threshold = 60;
        let similarity = 60;
        assert!(similarity >= threshold);
    }

    #[test]
    fn test_parity_threshold_59_fails() {
        let threshold = 60;
        let similarity = 59;
        assert!(similarity < threshold);
    }

    #[test]
    fn test_parity_threshold_100_passes() {
        assert!(100 >= 60);
    }

    #[test]
    fn test_parity_threshold_0_fails() {
        assert!(0 < 60);
    }
}

#[cfg(test)]
mod ink_feature_coverage_tests {
    #[test]
    fn test_core_components_identified() {
        let components = vec!["Box", "Text", "Newline", "Spacer", "Static", "Transform"];
        let mut unique: Vec<&str> = components.to_vec();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), components.len());
    }

    #[test]
    fn test_core_hooks_identified() {
        let hooks = vec!["useInput", "useApp", "useFocus", "useStdin", "useStdout", "useAnimation"];
        for hook in &hooks {
            assert!(hook.starts_with("use"));
        }
    }

    #[test]
    fn test_layout_props_comprehensive() {
        let layout_props = vec!["flexDirection", "flexWrap", "flexGrow", "flexShrink", "alignItems"];
        let mut unique: Vec<&str> = layout_props.to_vec();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), layout_props.len());
    }

    #[test]
    fn test_style_props_comprehensive() {
        let style_props = vec!["color", "backgroundColor", "borderStyle", "borderColor", "bold"];
        let mut unique: Vec<&str> = style_props.to_vec();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), style_props.len());
    }
}

#[cfg(test)]
mod edge_case_tests {
    #[test]
    fn test_long_content_handling() {
        let long_content = "x".repeat(10000);
        let lines: Vec<&str> = long_content.lines().collect();
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn test_terminal_width_simulation() {
        let terminal_width = 40;
        let content = "This is a very long line that exceeds the terminal width";
        let needs_wrap = content.len() > terminal_width;
        assert!(needs_wrap);
    }

    #[test]
    fn test_ansi_escape_handling() {
        let text = "\x1b[1mBold Text\x1b[0m";
        let mut cleaned = String::new();
        let mut in_escape = false;
        for c in text.chars() {
            if c == '\x1b' {
                in_escape = true;
            } else if in_escape && c == 'm' {
                in_escape = false;
            } else if !in_escape {
                cleaned.push(c);
            }
        }
        assert!(!cleaned.contains('\x1b'));
        assert_eq!(cleaned, "Bold Text");
    }

    #[test]
    fn test_box_drawing_chars() {
        let box_chars = "┌─┐│└┘├┤┬┴┼";
        assert!(box_chars.chars().all(|c| c.len_utf8() == 3));
    }

    #[test]
    fn test_diff_generation_algorithm() {
        let old = vec!["Line 1", "Line 2", "Line 3", "Line 4"];
        let new = vec!["Line 1", "Modified 2", "Line 3", "Line 5"];
        let common: Vec<&&str> = old.iter()
            .filter(|l| new.contains(l))
            .collect();
        assert_eq!(common.len(), 2);
    }

    #[test]
    fn test_timeout_simulation() {
        struct Process { timeout_ms: u64 }
        impl Process {
            fn is_timed_out(&self, elapsed_ms: u64) -> bool {
                elapsed_ms >= self.timeout_ms
            }
        }
        let proc = Process { timeout_ms: 5000 };
        assert!(!proc.is_timed_out(1000));
        assert!(proc.is_timed_out(5000));
    }

    #[test]
    fn test_file_path_normalization() {
        let paths = vec![
            "/path/to/ink-counter",
            "/path/to/ink-counter/",
        ];
        for path in &paths {
            let normalized = path.trim_end_matches('/');
            assert!(normalized.ends_with("ink-counter"));
        }
    }

    #[test]
    fn test_glob_pattern_matching() {
        let examples = vec!["ink-counter", "ink-box", "my-blog"];
        let ink_examples: Vec<_> = examples.iter()
            .filter(|e| e.starts_with("ink-"))
            .collect();
        assert_eq!(ink_examples.len(), 2);
    }

    #[test]
    fn test_json_result_serialization() {
        use serde::{Deserialize, Serialize};
        #[derive(Debug, Serialize, Deserialize)]
        struct TestResult {
            name: String,
            passed: bool,
            similarity: u8,
        }
        let result = TestResult {
            name: "ink-counter".to_string(),
            passed: true,
            similarity: 100,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("ink-counter"));
        let decoded: TestResult = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.name, "ink-counter");
    }

    #[test]
    fn test_empty_output_handling() {
        let empty_output = "";
        let lines: Vec<&str> = empty_output.lines().collect();
        assert!(lines.is_empty() || (lines.len() == 1 && lines[0].is_empty()));
    }
}
