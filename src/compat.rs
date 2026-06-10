//! Ink Compatibility Validation
//! 
//! Warns users when they use unsupported or partially-supported Ink props.
//! Only active in debug mode - zero overhead in production.

use std::collections::HashSet;

/// All Ink props supported by TuiBridge (Box components)
pub static SUPPORTED_BOX_PROPS: &[&str] = &[
    // Flex
    "flexDirection", "alignItems", "alignSelf", "justifyContent", "flexWrap",
    "flexGrow", "flexShrink", "flexBasis",
    // Spacing
    "margin", "marginTop", "marginBottom", "marginLeft", "marginRight",
    "marginX", "marginY",
    "padding", "paddingTop", "paddingBottom", "paddingLeft", "paddingRight",
    "paddingX", "paddingY",
    // Gap (supports both Ink 6 and Ink 7 names)
    "gap", "gapX", "gapY", "columnGap", "rowGap",
    // Borders
    "borderStyle", "borderColor", "borderDimColor",
    "borderTop", "borderBottom", "borderLeft", "borderRight",
    // Dimensions
    "width", "height", "minWidth", "maxWidth", "minHeight", "maxHeight",
    // Position (absolute)
    "position", "display", "title",
    "top", "right", "bottom", "left",
    // Children
    "children",
];

/// All Ink props supported by TuiBridge (Text components)
pub static SUPPORTED_TEXT_PROPS: &[&str] = &[
    // Color
    "color", "backgroundColor",
    // Style
    "bold", "dimColor", "dim", "italic",
    "strikethrough", "underline", "inverse",
    "small",
    // Transform & Wrap (supports both Ink 6 and Ink 7 prop names)
    "transform", "textWrap", "wrap",
    // Children
    "children",
];

/// Props with partial support
pub static PARTIAL_PROPS: &[&str] = &[
    "textWrap",  // "scroll", "end", "middle", etc. fall back to "wrap"
    "wrap",      // "end", "middle", "truncate-*" modes fall back to basic wrap/truncate
    "borderDimColor", // DIM modifier, not separate color
    "borderTop", "borderBottom", "borderLeft", "borderRight", // Individual borders use combined borderStyle
];

/// TextWrap enum for text wrapping behavior
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TextWrap {
    /// Default wrapping behavior
    #[default]
    Wrap,
    /// Cut text at width
    Truncate,
    /// Show ellipsis (approximated with truncate)
    Ellipsis,
    /// Scroll behavior (falls back to Wrap in TuiBridge)
    Scroll,
}

impl TextWrap {
    /// Parse TextWrap from a string value
    pub fn from_str(s: &str) -> Self {
        match s {
            "wrap" => TextWrap::Wrap,
            "truncate" => TextWrap::Truncate,
            "ellipsis" => TextWrap::Ellipsis,
            "scroll" => TextWrap::Scroll,
            _ => TextWrap::Wrap,
        }
    }
}

/// Validate Box props and return unsupported ones
pub fn validate_box_props(props: &HashSet<String>) -> Vec<String> {
    props.iter()
        .filter(|p| !SUPPORTED_BOX_PROPS.contains(&p.as_str()))
        .cloned()
        .collect()
}

/// Validate Text props and return unsupported ones
pub fn validate_text_props(props: &HashSet<String>) -> Vec<String> {
    props.iter()
        .filter(|p| !SUPPORTED_TEXT_PROPS.contains(&p.as_str()))
        .cloned()
        .collect()
}

/// Check if a prop has partial support
pub fn is_partial_prop(prop: &str) -> bool {
    PARTIAL_PROPS.contains(&prop)
}

/// Log warnings for unsupported props (only in debug mode)
pub fn warn_unsupported_props(unsupported: &[String], component: &str) {
    if unsupported.is_empty() {
        return;
    }
    
    for prop in unsupported {
        if is_partial_prop(prop) {
            let note = match prop.as_str() {
                "textWrap" => "(scroll falls back to wrap)",
                "borderDimColor" => "(uses DIM modifier)",
                _ => "",
            };
            tracing::warn!(
                "Partial support: '{}' on <{}> {}",
                prop, component, note
            );
        } else {
            tracing::warn!(
                "Unsupported prop '{}' on <{}>",
                prop, component
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_wrap_from_str() {
        assert_eq!(TextWrap::from_str("wrap"), TextWrap::Wrap);
        assert_eq!(TextWrap::from_str("truncate"), TextWrap::Truncate);
        assert_eq!(TextWrap::from_str("ellipsis"), TextWrap::Ellipsis);
        assert_eq!(TextWrap::from_str("scroll"), TextWrap::Scroll);
        assert_eq!(TextWrap::from_str("unknown"), TextWrap::Wrap);
    }

    #[test]
    fn test_validate_box_props() {
        let mut props = HashSet::new();
        props.insert("padding".to_string());
        props.insert("unknown".to_string());
        props.insert("color".to_string()); // color is for Text, not Box
        
        let unsupported = validate_box_props(&props);
        assert_eq!(unsupported.len(), 2);
        assert!(unsupported.contains(&"unknown".to_string()));
        assert!(unsupported.contains(&"color".to_string()));
    }

    #[test]
    fn test_validate_text_props() {
        let mut props = HashSet::new();
        props.insert("bold".to_string());
        props.insert("unknown".to_string());
        props.insert("flexGrow".to_string()); // flexGrow is for Box, not Text
        
        let unsupported = validate_text_props(&props);
        assert_eq!(unsupported.len(), 2);
        assert!(unsupported.contains(&"unknown".to_string()));
        assert!(unsupported.contains(&"flexGrow".to_string()));
    }

    #[test]
    fn test_is_partial_prop() {
        assert!(is_partial_prop("textWrap"));
        assert!(is_partial_prop("borderDimColor"));
        assert!(!is_partial_prop("color"));
        assert!(!is_partial_prop("padding"));
    }
}
