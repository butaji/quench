//! # JSX Transpiler
//!
//! Transpiles JSX/TSX syntax to Rust builder patterns (e.g., Ratatui).

/// Transpiles JSX to Rust builder patterns.
#[derive(Debug)]
pub struct JsxTranspiler {
    /// Widget library to target
    library: WidgetLibrary,
}

/// Target widget library.
#[derive(Debug, Clone, Copy)]
pub enum WidgetLibrary {
    /// Ratatui (TUI)
    Ratatui,
    /// Iced (GUI)
    Iced,
    /// Custom
    Custom,
}

impl Default for JsxTranspiler {
    fn default() -> Self {
        Self::new(WidgetLibrary::Ratatui)
    }
}

impl JsxTranspiler {
    /// Create a new JSX transpiler.
    #[must_use]
    pub fn new(library: WidgetLibrary) -> Self {
        Self { library }
    }

    /// Transpile a JSX element.
    #[allow(unused)]
    pub fn transpile_element(&self, elem: &str) -> String {
        match self.library {
            WidgetLibrary::Ratatui => self.transpile_ratatui(elem),
            WidgetLibrary::Iced => self.transpile_iced(elem),
            WidgetLibrary::Custom => elem.to_string(),
        }
    }

    /// Transpile to Ratatui widgets.
    fn transpile_ratatui(&self, elem: &str) -> String {
        // Basic JSX to Ratatui translation
        let elem = elem.trim();

        // Parse tag name
        let (tag, rest) = if let Some(space) = elem.find(' ') {
            (&elem[..space], &elem[space..])
        } else if let Some(gt) = elem.find('>') {
            (&elem[..gt], &elem[gt..])
        } else {
            (elem.trim_end_matches('/'), "")
        };

        let tag = tag.trim_start_matches('<').trim_end_matches('/');

        // Translate common tags to Ratatui widgets
        let widget = match tag {
            "Block" => "Block::default()",
            "List" => "List::new(items)",
            "ListItem" => "ListItem::new(text)",
            "Paragraph" => "Paragraph::new(text)",
            "Text" => "Paragraph::new(text)",
            "Button" => "Button::default()",
            "Input" => "Paragraph::new(\"[input]\")",
            "VStack" => "Column::new()",
            "HStack" => "Row::new()",
            "Spacer" => "Constraint::new(0).expand()",
            _ => &format!("{tag}::default()"),
        };

        format!("{}.block()", widget)
    }

    /// Transpile to Iced widgets.
    fn transpile_iced(&self, elem: &str) -> String {
        let elem = elem.trim();
        let tag = elem.split_whitespace().next().unwrap_or("Text").trim_start_matches('<');

        match tag {
            "Text" => "text(\"\")".to_string(),
            "Button" => "Button::new(\"Click me\")".to_string(),
            "Column" => "Column::new()".to_string(),
            "Row" => "Row::new()".to_string(),
            _ => format!("{tag}::new()", tag = tag),
        }
    }

    /// Transpile JSX expression with props.
    #[allow(unused)]
    pub fn transpile_with_props(&self, elem: &str, props: &[(&str, &str)]) -> String {
        let mut result = self.transpile_element(elem);

        for (key, value) in props {
            result.push_str(&format!(".{}({})", key, value));
        }

        result
    }
}
