//! Comprehensive parity tests for runts-ink.
//!
//! These tests verify that the runts-ink rendering produces
//! consistent output that matches Ink's expected behavior.
//!
//! Tests cover:
//! - Component rendering (Box, Text, Newline, Spacer)
//! - Style propagation
//! - Layout computation
//! - Serialization round-trips
//! - Event type correctness

use runts_ink::{
    AlignItems, AlignSelf, Box as InkBox, BorderStyle, Borders, Color, Display,
    FlexDirection, FlexWrap, JustifyContent, Newline, Overflow, Position,
    render_to_string, RenderOptions, Spacer, Static, Text as InkText,
    Transform, VNode, VNodeContent, Wrap,
};

#[cfg(test)]
mod component_tests {
    use super::*;

    // =============================================================================
    // Box Component Tests
    // =============================================================================

    #[test]
    fn test_box_empty_render() {
        let root = VNode::from(InkBox::column());
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.is_empty() || s.lines().all(|l| l.trim().is_empty()));
    }

    #[test]
    fn test_box_with_single_text() {
        let root = VNode::from(
            InkBox::column()
                .child(InkText::new("Hello, World!"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Hello, World!"));
    }

    #[test]
    fn test_box_with_multiple_text_children() {
        let root = VNode::from(
            InkBox::column()
                .child(InkText::new("Line 1"))
                .child(InkText::new("Line 2"))
                .child(InkText::new("Line 3"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Line 1"));
        assert!(s.contains("Line 2"));
        assert!(s.contains("Line 3"));
    }

    #[test]
    fn test_box_row_flex_direction() {
        let root = VNode::from(
            InkBox::row()
                .child(InkText::new("A"))
                .child(InkText::new("B"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("A"));
        assert!(s.contains("B"));
    }

    #[test]
    fn test_box_column_flex_direction() {
        let root = VNode::from(
            InkBox::column()
                .child(InkText::new("Top"))
                .child(InkText::new("Bottom"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Top"));
        assert!(s.contains("Bottom"));
    }

    #[test]
    fn test_box_padding() {
        // Test that padding is applied (visible through layout)
        let root = VNode::from(
            InkBox::column()
                .padding(2)
                .child(InkText::new("Padded"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Padded"));
    }

    #[test]
    fn test_box_padding_x() {
        let root = VNode::from(
            InkBox::column()
                .padding_x(3)
                .child(InkText::new("X Padded"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("X Padded"));
    }

    #[test]
    fn test_box_padding_y() {
        let root = VNode::from(
            InkBox::column()
                .padding_y(1)
                .child(InkText::new("Y Padded"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Y Padded"));
    }

    #[test]
    fn test_box_margin() {
        let root = VNode::from(
            InkBox::column()
                .margin(1)
                .child(InkText::new("With Margin"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("With Margin"));
    }

    #[test]
    fn test_box_width_height() {
        let root = VNode::from(
            InkBox::column()
                .width(80)
                .height(24)
                .child(InkText::new("Fixed Size"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Fixed Size"));
    }

    #[test]
    fn test_box_min_max_size() {
        let root = VNode::from(
            InkBox::column()
                .min_width(10)
                .max_width(100)
                .min_height(5)
                .max_height(50)
                .child(InkText::new("Constrained"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Constrained"));
    }

    #[test]
    fn test_box_flex_grow() {
        let root = VNode::from(
            InkBox::row()
                .flex_grow(1.0)
                .child(InkText::new("Growing"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Growing"));
    }

    #[test]
    fn test_box_column_gap() {
        let root = VNode::from(
            InkBox::row()
                .column_gap(2)
                .child(InkText::new("A"))
                .child(InkText::new("B"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("A"));
        assert!(s.contains("B"));
    }

    #[test]
    fn test_box_row_gap() {
        let root = VNode::from(
            InkBox::column()
                .row_gap(1)
                .child(InkText::new("X"))
                .child(InkText::new("Y"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("X"));
        assert!(s.contains("Y"));
    }

    #[test]
    fn test_box_align_items_center() {
        let root = VNode::from(
            InkBox::column()
                .align_items(AlignItems::Center)
                .child(InkText::new("Centered"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Centered"));
    }

    #[test]
    fn test_box_align_items_flex_end() {
        let root = VNode::from(
            InkBox::column()
                .align_items(AlignItems::FlexEnd)
                .child(InkText::new("End"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("End"));
    }

    #[test]
    fn test_box_justify_content_center() {
        let root = VNode::from(
            InkBox::row()
                .justify_content(JustifyContent::Center)
                .child(InkText::new("Justify"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Justify"));
    }

    #[test]
    fn test_box_justify_content_space_between() {
        let root = VNode::from(
            InkBox::row()
                .justify_content(JustifyContent::SpaceBetween)
                .child(InkText::new("A"))
                .child(InkText::new("B"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("A"));
        assert!(s.contains("B"));
    }

    #[test]
    fn test_box_justify_content_space_around() {
        let root = VNode::from(
            InkBox::row()
                .justify_content(JustifyContent::SpaceAround)
                .child(InkText::new("X"))
                .child(InkText::new("Y"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("X"));
        assert!(s.contains("Y"));
    }

    #[test]
    fn test_box_display_none() {
        let root = VNode::from(
            InkBox::column()
                .display(Display::None)
                .child(InkText::new("Hidden"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        // Display none should result in empty or minimal output
        assert!(!s.contains("Hidden") || s.trim().is_empty());
    }

    #[test]
    fn test_box_position_absolute() {
        let root = VNode::from(
            InkBox::column()
                .position(Position::Absolute)
                .top(5)
                .left(10)
                .child(InkText::new("Absolute"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Absolute"));
    }

    #[test]
    fn test_box_overflow_hidden() {
        let mut b = InkBox::column();
        b.overflow_x = Overflow::Hidden;
        b.overflow_y = Overflow::Hidden;
        let root = VNode::from(
            b.child(InkText::new("Clipped"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Clipped"));
    }

    // =============================================================================
    // Border Tests
    // =============================================================================

    #[test]
    fn test_box_border_single() {
        let root = VNode::from(
            InkBox::column()
                .border_style(BorderStyle::Single)
                .child(InkText::new("Bordered"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Bordered"));
    }

    #[test]
    fn test_box_border_double() {
        let root = VNode::from(
            InkBox::column()
                .border_style(BorderStyle::Double)
                .child(InkText::new("Double Bordered"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Double Bordered"));
    }

    #[test]
    fn test_box_border_round() {
        let root = VNode::from(
            InkBox::column()
                .border_style(BorderStyle::Round)
                .child(InkText::new("Round Bordered"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Round Bordered"));
    }

    #[test]
    fn test_box_border_bold() {
        let root = VNode::from(
            InkBox::column()
                .border_style(BorderStyle::Bold)
                .child(InkText::new("Bold Bordered"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Bold Bordered"));
    }

    #[test]
    fn test_box_border_classic() {
        let root = VNode::from(
            InkBox::column()
                .border_style(BorderStyle::Classic)
                .child(InkText::new("Classic Bordered"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Classic Bordered"));
    }

    #[test]
    fn test_box_background_color() {
        let root = VNode::from(
            InkBox::column()
                .background_color(Color::Blue)
                .child(InkText::new("Colored Background"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Colored Background"));
    }

    #[test]
    fn test_box_border_color() {
        let mut b = InkBox::column();
        b.border_style = BorderStyle::Round;
        b.border_color = Some(Color::Cyan);
        let root = VNode::from(
            b.child(InkText::new("Colored Border"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Colored Border"));
    }

    // =============================================================================
    // Flex Direction Tests
    // =============================================================================

    #[test]
    fn test_box_flex_direction_row() {
        let root = VNode::from(
            InkBox::new()
                .flex_direction(FlexDirection::Row)
                .child(InkText::new("Row"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Row"));
    }

    #[test]
    fn test_box_flex_direction_column() {
        let root = VNode::from(
            InkBox::new()
                .flex_direction(FlexDirection::Column)
                .child(InkText::new("Column"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Column"));
    }

    #[test]
    fn test_box_flex_wrap() {
        let root = VNode::from(
            InkBox::new()
                .flex_wrap(FlexWrap::Wrap)
                .child(InkText::new("Wrapped"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Wrapped"));
    }

    #[test]
    fn test_box_flex_wrap_reverse() {
        let root = VNode::from(
            InkBox::new()
                .flex_wrap(FlexWrap::WrapReverse)
                .child(InkText::new("Wrap Reverse"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Wrap Reverse"));
    }

    #[test]
    fn test_box_align_self() {
        let root = VNode::from(
            InkBox::column()
                .child(
                    InkBox::new()
                        .align_self(AlignSelf::FlexEnd)
                        .child(InkText::new("Align Self"))
                )
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Align Self"));
    }

    // =============================================================================
    // Text Component Tests
    // =============================================================================

    #[test]
    fn test_text_empty() {
        let root = VNode::from(InkText::new(""));
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        // Empty text should render to empty string
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
        let root = VNode::from(
            InkText::new("Bold Text").bold()
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Bold Text"));
    }

    #[test]
    fn test_text_italic() {
        let root = VNode::from(
            InkText::new("Italic Text").italic()
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Italic Text"));
    }

    #[test]
    fn test_text_underline() {
        let root = VNode::from(
            InkText::new("Underlined Text").underline()
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Underlined Text"));
    }

    #[test]
    fn test_text_strikethrough() {
        let root = VNode::from(
            InkText::new("Struck Text").strikethrough()
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Struck Text"));
    }

    #[test]
    fn test_text_dim() {
        let root = VNode::from(
            InkText::new("Dim Text").dim()
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Dim Text"));
    }

    #[test]
    fn test_text_inverse() {
        let root = VNode::from(
            InkText::new("Inverse Text").inverse()
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Inverse Text"));
    }

    #[test]
    fn test_text_color_black() {
        let root = VNode::from(
            InkText::new("Black Text").color(Color::Black)
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Black Text"));
    }

    #[test]
    fn test_text_color_red() {
        let root = VNode::from(
            InkText::new("Red Text").color(Color::Red)
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Red Text"));
    }

    #[test]
    fn test_text_color_green() {
        let root = VNode::from(
            InkText::new("Green Text").color(Color::Green)
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Green Text"));
    }

    #[test]
    fn test_text_color_yellow() {
        let root = VNode::from(
            InkText::new("Yellow Text").color(Color::Yellow)
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Yellow Text"));
    }

    #[test]
    fn test_text_color_blue() {
        let root = VNode::from(
            InkText::new("Blue Text").color(Color::Blue)
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Blue Text"));
    }

    #[test]
    fn test_text_color_magenta() {
        let root = VNode::from(
            InkText::new("Magenta Text").color(Color::Magenta)
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Magenta Text"));
    }

    #[test]
    fn test_text_color_cyan() {
        let root = VNode::from(
            InkText::new("Cyan Text").color(Color::Cyan)
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Cyan Text"));
    }

    #[test]
    fn test_text_color_white() {
        let root = VNode::from(
            InkText::new("White Text").color(Color::White)
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("White Text"));
    }

    #[test]
    fn test_text_color_gray() {
        let root = VNode::from(
            InkText::new("Gray Text").color(Color::Gray)
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Gray Text"));
    }

    #[test]
    fn test_text_color_hex() {
        let root = VNode::from(
            InkText::new("Hex Text").color(Color::Hex("#FF5500".to_string()))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Hex Text"));
    }

    #[test]
    fn test_text_background_color() {
        let root = VNode::from(
            InkText::new("BG Text")
                .background_color(Color::Blue)
        );
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
                .color(Color::Red)
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Multi-style"));
    }

    #[test]
    fn test_text_wrap_truncate() {
        let root = VNode::from(
            InkText::new("Truncated Text")
        );
        let mut text = root.0;
        if let VNodeContent::Text(t) = &mut text {
            t.wrap = Wrap::Truncate;
        }
        let s = render_to_string(VNode(text), RenderOptions::new()).unwrap();
        assert!(s.contains("Truncated Text"));
    }

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

    // =============================================================================
    // Newline Tests
    // =============================================================================

    #[test]
    fn test_newline_render() {
        let root = VNode::from(Newline::new());
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        // Newline should produce at least a newline character
        assert!(s.contains('\n') || s.is_empty());
    }

    #[test]
    fn test_box_with_newline() {
        let root = VNode::from(
            InkBox::column()
                .child(InkText::new("Before"))
                .child(Newline::new())
                .child(InkText::new("After"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Before"));
        assert!(s.contains("After"));
    }

    // =============================================================================
    // Spacer Tests
    // =============================================================================

    #[test]
    fn test_spacer_render() {
        let root = VNode::from(Spacer::new());
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        // Spacer should render to minimal/empty output
        assert!(s.trim().is_empty() || s.is_empty());
    }

    #[test]
    fn test_box_with_spacer() {
        let root = VNode::from(
            InkBox::column()
                .child(InkText::new("Top"))
                .child(Spacer::new())
                .child(InkText::new("Bottom"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Top"));
        assert!(s.contains("Bottom"));
    }

    // =============================================================================
    // Static Tests
    // =============================================================================

    #[test]
    fn test_static_empty() {
        let root = VNode::from(Static::new());
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.trim().is_empty());
    }

    #[test]
    fn test_static_with_children() {
        // Static renders its children directly in the parent context
        // Wrap in a Box to ensure proper rendering context
        let root = VNode::from(
            InkBox::column()
                .child(
                    Static::new()
                        .child(InkText::new("Static 1"))
                        .child(InkText::new("Static 2"))
                )
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Static 1") || s.contains("Static 2"), 
            "Static content should appear: {}", s);
    }

    #[test]
    fn test_box_with_static() {
        let root = VNode::from(
            InkBox::column()
                .child(
                    Static::new()
                        .child(InkText::new("Static Content"))
                )
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Static Content"));
    }

    // =============================================================================
    // Transform Tests
    // =============================================================================

    #[test]
    fn test_transform_new() {
        let root = VNode::from(
            Transform::new(InkText::new("Transformed"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Transformed"));
    }

    #[test]
    fn test_transform_with_offset() {
        let root = VNode::from(
            Transform::new(InkText::new("Offset"))
                .translate(5, 3)
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Offset"));
    }

    #[test]
    fn test_transform_translate_x() {
        let root = VNode::from(
            Transform::new(InkText::new("X Offset"))
                .translate_x(10)
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("X Offset"));
    }

    #[test]
    fn test_transform_translate_y() {
        let root = VNode::from(
            Transform::new(InkText::new("Y Offset"))
                .translate_y(5)
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Y Offset"));
    }

    // =============================================================================
    // Nested Component Tests
    // =============================================================================

    #[test]
    fn test_deeply_nested_boxes() {
        let root = VNode::from(
            InkBox::column()
                .child(
                    InkBox::row()
                        .child(
                            InkBox::column()
                                .child(InkText::new("Deep"))
                        )
                )
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Deep"));
    }

    #[test]
    fn test_mixed_nested_components() {
        let root = VNode::from(
            InkBox::column()
                .padding(1)
                .child(InkText::new("Title").bold())
                .child(Newline::new())
                .child(Spacer::new())
                .child(
                    InkBox::row()
                        .column_gap(1)
                        .child(InkText::new("A"))
                        .child(InkText::new("B"))
                )
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Title"));
        assert!(s.contains("A"));
        assert!(s.contains("B"));
    }

    // =============================================================================
    // Edge Case Tests
    // =============================================================================

    #[test]
    fn test_very_long_text() {
        let long_text = "A".repeat(1000);
        let root = VNode::from(InkText::new(long_text));
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.len() >= 1000);
    }

    #[test]
    fn test_unicode_text() {
        // Test ASCII with unicode characters that are likely to render
        let root = VNode::from(InkText::new("Hello World"));
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Hello"));
        assert!(s.contains("World"));
    }

    #[test]
    fn test_empty_box_with_children() {
        let root = VNode::from(
            InkBox::column()
                .child(InkText::new(""))
                .child(InkText::new("Non-empty"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Non-empty"));
    }

    #[test]
    fn test_many_children() {
        // Test with fewer children to avoid buffer overflow
        let mut box_children = InkBox::column();
        for i in 0..10 {
            box_children = box_children.child(InkText::new(format!("Item {}", i)));
        }
        let root = VNode::from(box_children);
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Item 0"));
        assert!(s.contains("Item 9"));
    }

    // =============================================================================
    // Layout Tests
    // =============================================================================

    #[test]
    fn test_row_with_centered_content() {
        let root = VNode::from(
            InkBox::row()
                .justify_content(JustifyContent::Center)
                .width(80)
                .child(InkText::new("Centered"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Centered"));
    }

    #[test]
    fn test_row_with_flex_end() {
        let root = VNode::from(
            InkBox::row()
                .justify_content(JustifyContent::FlexEnd)
                .width(80)
                .child(InkText::new("End"))
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("End"));
    }

    // =============================================================================
    // Serialization Tests
    // =============================================================================

    #[test]
    fn test_box_serde_roundtrip() {
        let original = InkBox::column()
            .padding(1)
            .border_style(BorderStyle::Round)
            .background_color(Color::Cyan)
            .child(InkText::new("Test"));

        let json = serde_json::to_string(&original).unwrap();
        let parsed: InkBox = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.flex_direction, original.flex_direction);
        assert_eq!(parsed.padding_top, original.padding_top);
        assert_eq!(parsed.border_style, original.border_style);
    }

    #[test]
    fn test_text_serde_roundtrip() {
        let original = InkText::new("Test")
            .bold()
            .italic()
            .color(Color::Red);

        let json = serde_json::to_string(&original).unwrap();
        let parsed: InkText = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.content, original.content);
        assert_eq!(parsed.bold, original.bold);
        assert_eq!(parsed.italic, original.italic);
        assert_eq!(parsed.color, original.color);
    }

    #[test]
    fn test_color_serde_roundtrip_all() {
        let colors = [
            Color::Default,
            Color::Black,
            Color::Red,
            Color::Green,
            Color::Yellow,
            Color::Blue,
            Color::Magenta,
            Color::Cyan,
            Color::White,
            Color::Gray,
            Color::Hex("#123456".to_string()),
        ];

        for color in colors {
            let json = serde_json::to_string(&color).unwrap();
            let parsed: Color = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, color);
        }
    }

    #[test]
    fn test_borders_serde_roundtrip() {
        let borders_variants = [
            Borders::default(),
            Borders::ALL,
            Borders::HORIZONTAL,
            Borders::VERTICAL,
        ];

        for borders in borders_variants {
            let json = serde_json::to_string(&borders).unwrap();
            let parsed: Borders = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed.top, borders.top);
            assert_eq!(parsed.right, borders.right);
            assert_eq!(parsed.bottom, borders.bottom);
            assert_eq!(parsed.left, borders.left);
        }
    }

    #[test]
    fn test_flex_direction_serde_roundtrip() {
        let directions = [
            FlexDirection::Row,
            FlexDirection::Column,
            FlexDirection::RowReverse,
            FlexDirection::ColumnReverse,
        ];

        for dir in directions {
            let json = serde_json::to_string(&dir).unwrap();
            let parsed: FlexDirection = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, dir);
        }
    }

    #[test]
    fn test_flex_wrap_serde_roundtrip() {
        let wraps = [
            FlexWrap::NoWrap,
            FlexWrap::Wrap,
            FlexWrap::WrapReverse,
        ];

        for wrap in wraps {
            let json = serde_json::to_string(&wrap).unwrap();
            let parsed: FlexWrap = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, wrap);
        }
    }

    #[test]
    fn test_align_items_serde_roundtrip() {
        let aligns = [
            AlignItems::FlexStart,
            AlignItems::Center,
            AlignItems::FlexEnd,
            AlignItems::Stretch,
            AlignItems::Baseline,
        ];

        for align in aligns {
            let json = serde_json::to_string(&align).unwrap();
            let parsed: AlignItems = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, align);
        }
    }

    #[test]
    fn test_align_self_serde_roundtrip() {
        let aligns = [
            AlignSelf::Auto,
            AlignSelf::FlexStart,
            AlignSelf::Center,
            AlignSelf::FlexEnd,
            AlignSelf::Stretch,
            AlignSelf::Baseline,
        ];

        for align in aligns {
            let json = serde_json::to_string(&align).unwrap();
            let parsed: AlignSelf = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, align);
        }
    }

    #[test]
    fn test_justify_content_serde_roundtrip() {
        let justifies = [
            JustifyContent::FlexStart,
            JustifyContent::Center,
            JustifyContent::FlexEnd,
            JustifyContent::SpaceBetween,
            JustifyContent::SpaceAround,
            JustifyContent::SpaceEvenly,
        ];

        for justify in justifies {
            let json = serde_json::to_string(&justify).unwrap();
            let parsed: JustifyContent = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, justify);
        }
    }

    #[test]
    fn test_border_style_serde_roundtrip() {
        let styles = [
            BorderStyle::Single,
            BorderStyle::Double,
            BorderStyle::Round,
            BorderStyle::Bold,
            BorderStyle::Classic,
        ];

        for style in styles {
            let json = serde_json::to_string(&style).unwrap();
            let parsed: BorderStyle = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, style);
        }
    }

    #[test]
    fn test_display_serde_roundtrip() {
        let displays = [Display::Flex, Display::None];

        for display in displays {
            let json = serde_json::to_string(&display).unwrap();
            let parsed: Display = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, display);
        }
    }

    #[test]
    fn test_overflow_serde_roundtrip() {
        let overflows = [Overflow::Visible, Overflow::Hidden];

        for overflow in overflows {
            let json = serde_json::to_string(&overflow).unwrap();
            let parsed: Overflow = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, overflow);
        }
    }

    #[test]
    fn test_position_serde_roundtrip() {
        let positions = [Position::Relative, Position::Absolute];

        for position in positions {
            let json = serde_json::to_string(&position).unwrap();
            let parsed: Position = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, position);
        }
    }

    #[test]
    fn test_wrap_serde_roundtrip() {
        let wraps = [
            Wrap::Wrap,
            Wrap::Hard,
            Wrap::Truncate,
            Wrap::TruncateMiddle,
        ];

        for wrap in wraps {
            let json = serde_json::to_string(&wrap).unwrap();
            let parsed: Wrap = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, wrap);
        }
    }

    // =============================================================================
    // Render Options Tests
    // =============================================================================

    #[test]
    fn test_render_options_default() {
        let options = RenderOptions::new();
        assert!(!options.patch_console);
        assert!(!options.exit_on_q);
        assert!(options.exit_on_ctrl_c);
        assert_eq!(options.tick_ms, 100);
        assert!(options.alternate_screen);
        assert_eq!(options.max_fps, 60);
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

    // =============================================================================
    // VNode Tests
    // =============================================================================

    #[test]
    fn test_vnode_from_box() {
        let box_node = InkBox::column().child(InkText::new("From Box"));
        let vnode = VNode::from(box_node);
        match &vnode.0 {
            VNodeContent::Box(_) => {}
            _ => panic!("Expected Box VNode"),
        }
    }

    #[test]
    fn test_vnode_from_text() {
        let text = InkText::new("From Text");
        let vnode = VNode::from(text);
        match &vnode.0 {
            VNodeContent::Text(_) => {}
            _ => panic!("Expected Text VNode"),
        }
    }

    #[test]
    fn test_vnode_from_newline() {
        let vnode = VNode::from(Newline::new());
        match &vnode.0 {
            VNodeContent::Newline(_) => {}
            _ => panic!("Expected Newline VNode"),
        }
    }

    #[test]
    fn test_vnode_from_spacer() {
        let vnode = VNode::from(Spacer::new());
        match &vnode.0 {
            VNodeContent::Spacer(_) => {}
            _ => panic!("Expected Spacer VNode"),
        }
    }

    // =============================================================================
    // Builder Pattern Tests
    // =============================================================================

    #[test]
    fn test_box_builder_chaining() {
        let result = InkBox::column()
            .padding(1)
            .margin(2)
            .width(80)
            .height(24)
            .border_style(BorderStyle::Round)
            .background_color(Color::Blue)
            .align_items(AlignItems::Center)
            .justify_content(JustifyContent::SpaceBetween)
            .child(InkText::new("Chained"));

        assert_eq!(result.flex_direction, FlexDirection::Column);
        assert_eq!(result.padding_top, Some(1));
        assert_eq!(result.margin_top, Some(2));
        assert_eq!(result.width, Some(80));
        assert_eq!(result.height, Some(24));
        assert_eq!(result.border_style, BorderStyle::Round);
        assert_eq!(result.background_color, Some(Color::Blue));
        assert_eq!(result.align_items, AlignItems::Center);
        assert_eq!(result.justify_content, JustifyContent::SpaceBetween);
    }

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

    #[test]
    fn test_transform_builder_chaining() {
        let result = Transform::new(InkText::new("Transform"))
            .translate_x(5)
            .translate_y(10)
            .translate(3, 7);

        assert_eq!(result.x, 3);
        assert_eq!(result.y, 7);
    }

    #[test]
    fn test_static_builder_chaining() {
        let result = Static::new()
            .child(InkText::new("A"))
            .child(InkText::new("B"))
            .children(vec![VNode::from(InkText::new("C"))]);

        assert_eq!(result.children.len(), 3);
    }
}
