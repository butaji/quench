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
        let (tag, rest) = split_element_tag(elem);
        let widget = map_tag_to_widget(tag);
        apply_props_to_widget(self, &widget, rest)
    }

    /// Transpile to Iced widget construction.
    fn transpile_iced(&self, elem: &str) -> String {
        let elem = elem.trim();
        let (tag, _rest) = split_element_tag(elem);
        map_iced_tag(tag)
    }

    /// Apply Ratatui props to widget chain.
    fn apply_ratatui_props(&self, result: &mut String, props: &str) {
        let props = props.trim_start().trim_end_matches("/>");
        let props = props.trim_end_matches('>');

        for prop in props.split_whitespace() {
            if let Some((key, value)) = prop.split_once('=') {
                let setter =
                    build_prop_setter(key.trim(), value.trim_matches('"').trim_matches('\''));
                let _ = write!(result, "{setter}");
            }
        }
    }

    /// Transpile JSX children.
    #[must_use]
    pub fn transpile_children(&self, children: &str) -> Vec<String> {
        parse_jsx_children(children)
    }

    /// Extract props from JSX attributes.
    #[must_use]
    pub fn extract_props<'a>(&self, attrs: &'a str) -> Vec<(&'a str, &'a str)> {
        parse_jsx_props(attrs)
    }
}

fn split_element_tag(elem: &str) -> (&str, Option<&str>) {
    if let Some(space) = elem.find(' ') {
        (&elem[..space], Some(&elem[space..]))
    } else if let Some(gt) = elem.find('>') {
        (&elem[..gt], Some(&elem[gt..]))
    } else {
        (elem.trim_end_matches('/'), None)
    }
}

fn map_tag_to_widget(tag: &str) -> String {
    match tag {
        "Block" => "Block::default()".to_string(),
        "List" => "List::new(items)".to_string(),
        "ListItem" => "ListItem::new(text)".to_string(),
        "Paragraph" | "Text" => "Paragraph::new(text)".to_string(),
        "Button" => "Button::default()".to_string(),
        "Input" => "Paragraph::new(text)".to_string(),
        "VStack" => "Column::new()".to_string(),
        "HStack" => "Row::new()".to_string(),
        _ => format!("{tag}::default()"),
    }
}

fn apply_props_to_widget(transpiler: &JsxTranspiler, widget: &str, props: Option<&str>) -> String {
    let mut result = widget.to_string();
    if let Some(props_str) = props {
        transpiler.apply_ratatui_props(&mut result, props_str);
    }
    result
}

fn build_prop_setter(key: &str, value: &str) -> String {
    match key {
        "title" => format!(".title(\"{}\")", value),
        "borders" => ".block()".to_string(),
        "style" => format!(".style({})", value),
        "width" => format!(".width({})", value),
        "height" => format!(".height({})", value),
        "align" => format!(".alignment({})", value),
        _ => String::new(),
    }
}

fn map_iced_tag(tag: &str) -> String {
    match tag {
        "Button" => "button::Button::new(text)".to_string(),
        "Text" => "text::Text::new(text)".to_string(),
        "Column" => "Column::new()".to_string(),
        "Row" => "Row::new()".to_string(),
        "Container" => "Container::new(content)".to_string(),
        _ => format!("{tag}::new()"),
    }
}

fn parse_jsx_children(children: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut in_tag = false;
    let mut element_buffer = String::new();
    let mut pending_tag: Option<String> = None;
    let mut tag_content = String::new();

    for c in children.chars() {
        if c == '<' {
            in_tag = true;
            tag_content.clear();
            tag_content.push(c);
        } else if c == '>' {
            tag_content.push(c);
            in_tag = false;

            let tag_str = tag_content.trim().trim_matches('<').trim().to_string();

            if tag_str.starts_with('/') {
                let closing = tag_str.trim_start_matches('/');
                if pending_tag.as_ref().is_some_and(|t| t == closing) {
                    element_buffer.push_str(&tag_content);
                    result.push(element_buffer.clone());
                    element_buffer.clear();
                    pending_tag = None;
                }
            } else if tag_str.ends_with('/') {
                element_buffer.push_str(&tag_content);
                result.push(element_buffer.clone());
                element_buffer.clear();
                pending_tag = None;
            } else {
                element_buffer.push_str(&tag_content);
                pending_tag = Some(tag_str);
            }
        } else if in_tag {
            tag_content.push(c);
        } else {
            element_buffer.push(c);
        }
    }

    result
}

fn parse_jsx_props(attrs: &str) -> Vec<(&str, &str)> {
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
