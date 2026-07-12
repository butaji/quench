//! test262 frontmatter parsing — no external YAML crate.
//!
//! Parses the /*--- ... ---*/ block in test262 test files inline.

/// Represents a negative test expectation.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Negative {
    /// "parse" or "runtime"
    pub phase: String,
    /// Error type name e.g. "SyntaxError"
    pub typ: String,
}

/// Parsed test262 metadata from frontmatter.
#[derive(Debug, Default, Clone)]
pub struct Test262Metadata {
    pub description: Option<String>,
    #[allow(dead_code)]
    pub esid: Option<String>,
    #[allow(dead_code)]
    pub info: Option<String>,
    pub flags: Vec<String>,
    pub includes: Vec<String>,
    pub features: Vec<String>,
    pub negative: Option<Negative>,
}

impl Test262Metadata {
    /// Parse the /*--- ... ---*/ block from test source.
    pub fn parse(source: &str) -> Option<Self> {
        let start = source.find("/*---")? + 5;
        let end = source[start..].find("---*/")? + start;
        let yaml = &source[start..end];
        parse_yaml(yaml)
    }

    #[allow(dead_code)]
    pub fn is_negative(&self) -> bool {
        self.negative.is_some()
    }
}

/// Parse a minimal YAML subset (test262 frontmatter).
fn parse_yaml(yaml: &str) -> Option<Test262Metadata> {
    let mut meta = Test262Metadata::default();
    let mut in_negative = false;
    // Key of a block-style list (`key:` with empty value, then `- item` lines).
    let mut block_list_key: Option<String> = None;

    for raw_line in yaml.lines() {
        let line = raw_line.trim();

        // Handle nested negative block
        if line == "negative:" {
            in_negative = true;
            block_list_key = None;
            meta.negative = Some(Negative {
                phase: String::new(),
                typ: String::new(),
            });
            continue;
        }
        // The negative block ends at the first non-indented line; test the RAW
        // line for leading whitespace (the trimmed line never has any).
        if in_negative
            && !raw_line.starts_with(char::is_whitespace)
            && !line.starts_with('#')
            && !line.is_empty()
        {
            in_negative = false;
        }

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Block-style YAML list item: `- value` following a `key:` with empty value.
        if let Some(key) = block_list_key.clone() {
            if let Some(item) = line.strip_prefix("- ") {
                let item = item.trim().trim_matches('"').trim_matches('\'').to_string();
                if !item.is_empty() {
                    match key.as_str() {
                        "flags" => meta.flags.push(item),
                        "includes" => meta.includes.push(item),
                        "features" => meta.features.push(item),
                        _ => {}
                    }
                }
                continue;
            }
            block_list_key = None;
        }

        // Indented inside negative block
        if in_negative {
            if let Some((key, val)) = line.trim().split_once(':') {
                match key.trim() {
                    "phase" => {
                        if let Some(n) = &mut meta.negative {
                            n.phase = val.trim().to_string();
                        }
                    }
                    "type" => {
                        if let Some(n) = &mut meta.negative {
                            n.typ = val.trim().to_string();
                        }
                    }
                    _ => {}
                }
            }
            continue;
        }

        if let Some((key, val)) = line.split_once(':') {
            let key = key.trim();
            let val = val.trim();

            match key {
                "description" => meta.description = Some(val.to_string()),
                "esid" => meta.esid = Some(val.to_string()),
                "info" => meta.info = Some(val.to_string()),
                "flags" | "includes" | "features" => {
                    if val.is_empty() {
                        // Block-style list: items follow as `- value` lines.
                        block_list_key = Some(key.to_string());
                    } else {
                        let items = parse_list(val);
                        match key {
                            "flags" => meta.flags = items,
                            "includes" => meta.includes = items,
                            _ => meta.features = items,
                        }
                    }
                }
                _ => {}
            }
        }
    }

    Some(meta)
}

/// Parse a YAML list: `[a, b, c]` or a single value.
fn parse_list(val: &str) -> Vec<String> {
    let val = val.trim();
    if val.starts_with('[') && val.ends_with(']') {
        val[1..val.len() - 1]
            .split(',')
            .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        let s = val.trim_matches('"').trim_matches('\'').to_string();
        if s.is_empty() {
            vec![]
        } else {
            vec![s]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_frontmatter() {
        let src = r#"/*---
description: addition returns a number
flags: [onlyStrict]
includes: [assert.js, sta.js]
negative:
  phase: runtime
  type: ReferenceError
---*/
1 + 1;
"#;
        let meta = Test262Metadata::parse(src).unwrap();
        assert_eq!(
            meta.description,
            Some("addition returns a number".to_string())
        );
        assert!(meta.flags.contains(&"onlyStrict".to_string()));
        assert_eq!(meta.includes, vec!["assert.js", "sta.js"]);
        assert!(meta.is_negative());
        let neg = meta.negative.unwrap();
        assert_eq!(neg.phase, "runtime");
        assert_eq!(neg.typ, "ReferenceError");
    }

    #[test]
    fn parse_negative_phase_and_type() {
        // Regression: the negative block must stay open for indented
        // `phase:`/`type:` lines (whitespace check on the raw line).
        let src = "/*---\ndescription: d\nnegative:\n  phase: parse\n  type: SyntaxError\n---*/\n";
        let meta = Test262Metadata::parse(src).unwrap();
        let neg = meta.negative.unwrap();
        assert_eq!(neg.phase, "parse");
        assert_eq!(neg.typ, "SyntaxError");
    }

    #[test]
    fn parse_negative_block_ends_at_top_level_key() {
        let src =
            "/*---\nnegative:\n  phase: runtime\n  type: TypeError\nflags: [noStrict]\n---*/\n";
        let meta = Test262Metadata::parse(src).unwrap();
        assert_eq!(meta.negative.unwrap().phase, "runtime");
        assert_eq!(meta.flags, vec!["noStrict"]);
    }

    #[test]
    fn parse_block_style_lists() {
        let src = "/*---\ndescription: block lists\nflags:\n  - noStrict\n  - generated\nfeatures:\n  - arrowFunctions\nincludes:\n  - assert.js\n---*/\n";
        let meta = Test262Metadata::parse(src).unwrap();
        assert_eq!(meta.flags, vec!["noStrict", "generated"]);
        assert_eq!(meta.features, vec!["arrowFunctions"]);
        assert_eq!(meta.includes, vec!["assert.js"]);
    }

    #[test]
    fn parse_minimal_frontmatter() {
        let src = "/*--- ---*/ 1 + 1;";
        let meta = Test262Metadata::parse(src).unwrap();
        assert!(meta.description.is_none());
        assert!(meta.flags.is_empty());
        assert!(!meta.is_negative());
    }

    #[test]
    fn parse_with_features() {
        let src = "/*---
description: test with features
features: [arrowFunctions, const]
---*/
const f = () => 1;
";
        let meta = Test262Metadata::parse(src).unwrap();
        assert_eq!(meta.features, vec!["arrowFunctions", "const"]);
    }

    #[test]
    fn parse_no_frontmatter() {
        let meta = Test262Metadata::parse("1 + 1;");
        assert!(meta.is_none());
    }
}
