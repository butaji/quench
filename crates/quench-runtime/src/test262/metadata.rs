//! test262 frontmatter parsing
//!
//! Parses the YAML frontmatter between /*--- and ---*/ in test262 test files.

use serde::Deserialize;

/// Represents a negative test expectation
#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
pub struct Negative {
    /// The phase where the error should occur (parse or runtime)
    pub phase: String,
    /// The expected error type
    #[serde(rename = "type")]
    pub typ: String,
}

/// Represents parsed test262 metadata from the frontmatter
#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
pub struct Test262Metadata {
    /// Human-readable description of the test
    pub description: Option<String>,
    /// ECMAScript identifier
    pub esid: Option<String>,
    /// Additional info about the test
    pub info: Option<String>,
    /// Test flags (e.g., onlyStrict, raw)
    #[serde(default)]
    pub flags: Vec<String>,
    /// Included helper files
    #[serde(default)]
    pub includes: Vec<String>,
    /// Features required by this test
    #[serde(default)]
    pub features: Vec<String>,
    /// Expected negative result
    pub negative: Option<Negative>,
}

impl Test262Metadata {
    /// Parse YAML frontmatter from test262 test source
    pub fn parse(source: &str) -> Option<Self> {
        // Find the /*--- marker
        let start = source.find("/*---")?;
        let start = start + 5; // Skip past /*---
        
        // Find the ---*/ closing marker
        let end = source.find("---*/")?;
        
        // Extract YAML content
        let yaml = &source[start..end];
        
        // Parse YAML using serde_yaml
        serde_yaml::from_str(yaml).ok()
    }
    
    /// Check if this is a negative test
    pub fn is_negative(&self) -> bool {
        self.negative.is_some()
    }
    
    /// Get the negative expectation (if any)
    pub fn negative(&self) -> Option<&Negative> {
        self.negative.as_ref()
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
        assert_eq!(meta.description, Some("addition returns a number".to_string()));
        assert!(meta.flags.contains(&"onlyStrict".to_string()));
        assert_eq!(meta.includes, vec!["assert.js", "sta.js"]);
        assert!(meta.is_negative());
        let neg = meta.negative().unwrap();
        assert_eq!(neg.phase, "runtime");
        assert_eq!(neg.typ, "ReferenceError");
    }

    #[test]
    fn parse_minimal_frontmatter() {
        // Empty YAML content between markers
        let src = r#"/*---
---*/ 1 + 1;
"#;
        let meta = Test262Metadata::parse(src);
        assert!(meta.is_some());
        let meta = meta.unwrap();
        assert!(meta.description.is_none());
        assert!(meta.flags.is_empty());
        assert!(!meta.is_negative());
    }

    #[test]
    fn parse_with_features() {
        let src = r#"/*---
description: test with features
features: [arrowFunctions, const]
---*/
const f = () => 1;
"#;
        let meta = Test262Metadata::parse(src).unwrap();
        assert_eq!(meta.features, vec!["arrowFunctions", "const"]);
    }

    #[test]
    fn parse_no_frontmatter() {
        let src = "1 + 1;";
        let meta = Test262Metadata::parse(src);
        assert!(meta.is_none());
    }
}
