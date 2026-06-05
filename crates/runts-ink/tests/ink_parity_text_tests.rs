//! Parity tests for Ink Text component.
//!
//! Tests Text rendering, styling properties (bold, italic, colors),
//! and text formatting features.

use runts_ink::{render_to_string, Color, RenderOptions, Text as InkText, VNode, VNodeContent, Wrap};

#[cfg(test)]
mod text_basic_tests {
    use super::*;

    #[test]
    fn test_text_empty() {
        let root = VNode::from(InkText::new(""));
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.trim().is_empty() || !s.contains('\n'));
    }

    #[test]
    fn test_text_with_content() {
        let root = VNode::from(InkText::new("Content"));
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Content"));
    }

    #[test]
    fn test_text_bold() {
        let root = VNode::from(InkText::new("Bold Text").bold());
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Bold Text"));
    }

    #[test]
    fn test_text_italic() {
        let root = VNode::from(InkText::new("Italic Text").italic());
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Italic Text"));
    }

    #[test]
    fn test_text_underline() {
        let root = VNode::from(InkText::new("Underlined Text").underline());
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Underlined Text"));
    }

    #[test]
    fn test_text_strikethrough() {
        let root = VNode::from(InkText::new("Struck Text").strikethrough());
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Struck Text"));
    }

    #[test]
    fn test_text_dim() {
        let root = VNode::from(InkText::new("Dim Text").dim());
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Dim Text"));
    }

    #[test]
    fn test_text_inverse() {
        let root = VNode::from(InkText::new("Inverse Text").inverse());
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Inverse Text"));
    }
}

#[cfg(test)]
mod text_color_tests {
    use super::*;

    #[test]
    fn test_text_color_black() {
        let root = VNode::from(InkText::new("Black Text").color(Color::Black));
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Black Text"));
    }

    #[test]
    fn test_text_color_red() {
        let root = VNode::from(InkText::new("Red Text").color(Color::Red));
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Red Text"));
    }

    #[test]
    fn test_text_color_green() {
        let root = VNode::from(InkText::new("Green Text").color(Color::Green));
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Green Text"));
    }

    #[test]
    fn test_text_color_yellow() {
        let root = VNode::from(InkText::new("Yellow Text").color(Color::Yellow));
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Yellow Text"));
    }

    #[test]
    fn test_text_color_blue() {
        let root = VNode::from(InkText::new("Blue Text").color(Color::Blue));
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Blue Text"));
    }

    #[test]
    fn test_text_color_magenta() {
        let root = VNode::from(InkText::new("Magenta Text").color(Color::Magenta));
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Magenta Text"));
    }

    #[test]
    fn test_text_color_cyan() {
        let root = VNode::from(InkText::new("Cyan Text").color(Color::Cyan));
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Cyan Text"));
    }

    #[test]
    fn test_text_color_white() {
        let root = VNode::from(InkText::new("White Text").color(Color::White));
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("White Text"));
    }

    #[test]
    fn test_text_color_gray() {
        let root = VNode::from(InkText::new("Gray Text").color(Color::Gray));
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Gray Text"));
    }

    #[test]
    fn test_text_color_hex() {
        let root = VNode::from(
            InkText::new("Hex Text").color(Color::Hex("#FF5500".to_string())),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Hex Text"));
    }

    #[test]
    fn test_text_background_color() {
        let root = VNode::from(InkText::new("BG Text").background_color(Color::Blue));
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("BG Text"));
    }

    #[test]
    fn test_text_multiple_styles() {
        let root = VNode::from(
            InkText::new("Multi-style")
                .bold()
                .italic()
                .underline()
                .color(Color::Red),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Multi-style"));
    }

    #[test]
    fn test_text_wrap_truncate() {
        let root = VNode::from(InkText::new("Truncated Text"));
        let mut text = root.0;
        if let VNodeContent::Text(t) = &mut text {
            t.wrap = Wrap::Truncate;
        }
        let s = render_to_string(VNode(text), RenderOptions::new()).unwrap();
        assert!(s.contains("Truncated Text"));
    }
}

#[cfg(test)]
mod text_has_styling_tests {
    use super::*;

    #[test]
    fn test_text_has_styling_true() {
        let t = InkText::new("Styled").bold().color(Color::Red);
        assert!(t.has_styling());
    }

    #[test]
    fn test_text_has_styling_false() {
        let t = InkText::new("Plain");
        assert!(!t.has_styling());
    }
}

#[cfg(test)]
mod text_builder_tests {
    use super::*;

    #[test]
    fn test_text_builder_chaining() {
        let result = InkText::new("Chained")
            .bold()
            .italic()
            .underline()
            .strikethrough()
            .dim()
            .inverse()
            .color(Color::Red)
            .background_color(Color::Blue);

        assert_eq!(result.content, "Chained");
        assert!(result.bold);
        assert!(result.italic);
        assert!(result.underline);
        assert!(result.strikethrough);
        assert!(result.dim_color);
        assert!(result.inverse);
        assert_eq!(result.color, Color::Red);
        assert_eq!(result.background_color, Color::Blue);
    }
}
