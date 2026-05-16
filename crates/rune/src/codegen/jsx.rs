//! # JSX Transpiler
//!
//! Transpiles JSX/TSX syntax to Rust widget construction patterns.

use std::fmt::Write as FmtWrite;

/// Widget library to transpile to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetLibrary {
    /// Ratatui TUI library
    Ratatui,
    /// Iced GUI library
    Iced,
}

impl Default for WidgetLibrary {
    fn default() -> Self {
        Self::Ratatui
    }
}

/// JSX transpiler for Rust UI libraries.
#[derive(Debug)]
pub struct JsxTranspiler {
    /// Target widget library
    library: WidgetLibrary,
}

impl JsxTranspiler {
    /// Create a new JSX transpiler.
    #[must_use]
    pub const fn new(library: WidgetLibrary) -> Self {
        Self { library }
    }

    /// Transpile a JSX element.
    #[must_use]
    pub fn transpile_element(&self, elem: &str) -> String {
        match self.library {
            WidgetLibrary::Ratatui => self.transpile_ratatui(elem),
            WidgetLibrary::Iced => self.transpile_iced(elem),
        }
    }

    /// Transpile to Ratatui widget construction.
    fn transpile_ratatui(&self, elem: &str) -> String {
        let elem = elem.trim();
        let (tag, _rest) = if let Some(space) = elem.find(' ') {
            (&elem[..space], Some(&elem[space..]))
        } else if let Some(gt) = elem.find('>') {
            (&elem[..gt], Some(&elem[gt..]))
        } else {
            (elem.trim_end_matches('/'), None)
        };

        let widget = match tag {
            "Block" => "Block::default()",
            "List" => "List::new(items)",
            "ListItem" => "ListItem::new(text)",
            "Paragraph" | "Text" => "Paragraph::new(text)",
            "Button" => "Button::default()",
            "Input" => "Paragraph::new(text)", // Simplified
            "VStack" => "Column::new()",
            "HStack" => "Row::new()",
            _ => &format!("{tag}::default()"),
        };

        format!("{widget}.block()")
    }

    /// Transpile to Iced widget construction.
    fn transpile_iced(&self, elem: &str) -> String {
        let elem = elem.trim();
        let (tag, _rest) = if let Some(space) = elem.find(' ') {
            (&elem[..space], Some(&elem[space..]))
        } else {
            (elem.trim_end_matches('/'), None)
        };

        match tag {
            "Button" => "button::Button::new(text)".to_string(),
            "Text" => "text::Text::new(text)".to_string(),
            "Column" => "Column::new()".to_string(),
            "Row" => "Row::new()".to_string(),
            "Container" => "Container::new(content)".to_string(),
            _ => format!("{tag}::new()"),
        }
    }

    /// Transpile an element with props.
    #[must_use]
    pub fn transpile_with_props(&self, elem: &str, props: &[(&str, &str)]) -> String {
        let base = self.transpile_element(elem);
        let mut result = base;

        for (key, value) in props {
            let _ = write!(result, ".{key}({value})");
        }

        result
    }

    /// Extract props from JSX attributes.
    #[must_use]
    pub fn extract_props<'a>(&self, attrs: &'a str) -> Vec<(&'a str, &'a str)> {
        let attrs = attrs.trim();
        if attrs.is_empty() {
            return Vec::new();
        }

        attrs
            .split_whitespace()
            .filter_map(|attr| {
                let parts: Vec<&str> = attr.split('=').collect();
                if parts.len() == 2 {
                    Some((parts[0], parts[1].trim_matches('"').trim_matches('\'')))
                } else {
                    None
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_element() {
        let transpiler = JsxTranspiler::new(WidgetLibrary::Ratatui);
        let result = transpiler.transpile_element("Block");
        assert!(result.contains("Block"));
    }

    #[test]
    fn test_props_extraction() {
        let transpiler = JsxTranspiler::new(WidgetLibrary::Ratatui);
        let props = transpiler.extract_props(r#"title="Hello" borders="single"#);
        assert_eq!(props.len(), 2);
        assert_eq!(props[0], ("title", "Hello"));
    }
}
