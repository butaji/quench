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
        let (tag, rest) = if let Some(space) = elem.find(' ') {
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
            "Input" => "Paragraph::new(text)",
            "VStack" => "Column::new()",
            "HStack" => "Row::new()",
            _ => &format!("{tag}::default()"),
        };

        let mut result = widget.to_string();

        // Apply props if present
        if let Some(props_str) = rest {
            self.apply_ratatui_props(&mut result, props_str);
        }

        result
    }

    /// Apply Ratatui props to widget chain.
    fn apply_ratatui_props(&self, result: &mut String, props: &str) {
        let props = props.trim_start().trim_end_matches("/>");
        let props = props.trim_end_matches('>');

        for prop in props.split_whitespace() {
            let prop = prop.trim();
            if prop.is_empty() || prop.starts_with('<') {
                continue;
            }

            if let Some((key, value)) = prop.split_once('=') {
                let key = key.trim();
                let value = value.trim_matches('"').trim_matches('\'');

                let setter = match key {
                    "title" => format!(".title(\"{}\")", value),
                    "borders" => ".block()".to_string(),
                    "style" => format!(".style({})", value),
                    "width" => format!(".width({})", value),
                    "height" => format!(".height({})", value),
                    "align" => format!(".alignment({})", value),
                    _ => String::new(),
                };

                let _ = write!(result, "{setter}");
            }
        }
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

    /// Transpile JSX children.
    #[must_use]
    pub fn transpile_children(&self, children: &str) -> Vec<String> {
        let mut result = Vec::new();
        let mut element_content = String::new();
        let mut in_tag = false;
        let mut pending_open_tag: Option<String> = None;

        for c in children.chars() {
            if !in_tag {
                if c == '<' {
                    // Start of a new tag - begin collecting element
                    in_tag = true;
                    // Clear any previous element content
                    element_content.clear();
                    element_content.push(c);
                }
                // Skip text content between tags - we don't want to include it
            } else {
                // Inside a tag
                element_content.push(c);

                if c == '>' {
                    // Determine tag type - find the content between < and this >
                    let tag_str = if let Some(end_idx) = element_content.find('>') {
                        element_content[1..end_idx].to_string()
                    } else {
                        element_content.trim().trim_start_matches('<').to_string()
                    };

                    if tag_str.starts_with('/') {
                        // Closing tag
                        let closing_name = tag_str.trim_start_matches('/');
                        if pending_open_tag.as_ref().is_some_and(|n| n == closing_name) {
                            // Match found - complete the element
                            // element_content now has full element, push it
                            let element = element_content.trim().to_string();
                            result.push(element);
                            element_content.clear();
                            pending_open_tag = None;
                        }
                    } else if tag_str.ends_with('/') {
                        // Self-closing tag - element is complete
                        let element = element_content.trim().to_string();
                        result.push(element);
                        element_content.clear();
                        pending_open_tag = None;
                    } else {
                        // Opening tag - remember it
                        pending_open_tag = Some(tag_str.clone());
                    }

                    in_tag = false;
                }
            }
        }

        // Handle any remaining content (unclosed element)
        if !element_content.trim().is_empty() {
            result.push(element_content.trim().to_string());
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

    #[test]
    fn test_children_parsing() {
        let transpiler = JsxTranspiler::new(WidgetLibrary::Ratatui);
        let children = "<Text>Hello</Text><Text>World</Text>";
        let result = transpiler.transpile_children(children);
        assert_eq!(result.len(), 2);
    }
}
