//! Unit tests for runts-ink components.
//!
//! These tests verify that the Ink-compatible Rust types
//! behave correctly and produce consistent output.

use runts_ink::{
    AlignItems, AlignSelf, Box, BorderStyle, Borders, Color, Display,
    FlexDirection, FlexWrap, JustifyContent, Newline, Overflow, Position, Spacer, Static,
    Text, Transform, Wrap,
};

#[test]
fn test_box_default_values() {
    let b = Box::default();
    
    // Layout defaults
    assert_eq!(b.flex_direction, FlexDirection::Row);
    assert_eq!(b.flex_wrap, FlexWrap::NoWrap);
    assert_eq!(b.flex_grow, 0.0);
    assert_eq!(b.flex_shrink, 1.0);
    
    // Size defaults
    assert_eq!(b.width, None);
    assert_eq!(b.height, None);
    
    // Display defaults
    assert_eq!(b.display, Display::Flex);
    assert_eq!(b.position, Position::Relative);
    
    // No children by default
    assert!(b.children.is_empty());
}

#[test]
fn test_box_column() {
    let b = Box::column();
    assert_eq!(b.flex_direction, FlexDirection::Column);
}

#[test]
fn test_box_row() {
    let b = Box::row();
    assert_eq!(b.flex_direction, FlexDirection::Row);
}

#[test]
fn test_box_padding() {
    let b = Box::column().padding(2);
    assert_eq!(b.padding_top, Some(2));
    assert_eq!(b.padding_right, Some(2));
    assert_eq!(b.padding_bottom, Some(2));
    assert_eq!(b.padding_left, Some(2));
}

#[test]
fn test_box_padding_x() {
    let b = Box::column().padding_x(3);
    assert_eq!(b.padding_left, Some(3));
    assert_eq!(b.padding_right, Some(3));
    assert_eq!(b.padding_top, None);
    assert_eq!(b.padding_bottom, None);
}

#[test]
fn test_box_padding_y() {
    let b = Box::column().padding_y(1);
    assert_eq!(b.padding_top, Some(1));
    assert_eq!(b.padding_bottom, Some(1));
    assert_eq!(b.padding_left, None);
    assert_eq!(b.padding_right, None);
}

#[test]
fn test_box_margin() {
    let b = Box::column().margin(2);
    assert_eq!(b.margin_top, Some(2));
    assert_eq!(b.margin_right, Some(2));
    assert_eq!(b.margin_bottom, Some(2));
    assert_eq!(b.margin_left, Some(2));
}

#[test]
fn test_box_width_height() {
    let b = Box::column().width(80).height(24);
    assert_eq!(b.width, Some(80));
    assert_eq!(b.height, Some(24));
}

#[test]
fn test_box_min_max_size() {
    let b = Box::column()
        .min_width(10)
        .max_width(100)
        .min_height(5)
        .max_height(50);
    assert_eq!(b.min_width, Some(10));
    assert_eq!(b.max_width, Some(100));
    assert_eq!(b.min_height, Some(5));
    assert_eq!(b.max_height, Some(50));
}

#[test]
fn test_box_flex_grow() {
    let b = Box::column().flex_grow(1.0);
    assert_eq!(b.flex_grow, 1.0);
}

#[test]
fn test_box_gaps() {
    let b = Box::column().column_gap(2).row_gap(3);
    assert_eq!(b.column_gap, Some(2));
    assert_eq!(b.row_gap, Some(3));
}

#[test]
fn test_box_alignment() {
    let b = Box::column();
    // Box Default sets align_items to Stretch (per Ink/Yoga behavior for cross-axis stretch)
    assert_eq!(b.align_items, AlignItems::Stretch);
    assert_eq!(b.align_self, AlignSelf::Auto);
    assert_eq!(b.justify_content, JustifyContent::FlexStart);
    
    // Test that fields can be set
    let mut b = Box::column();
    b.align_items = AlignItems::Center;
    b.align_self = AlignSelf::FlexEnd;
    b.justify_content = JustifyContent::Center;
    
    assert_eq!(b.align_items, AlignItems::Center);
    assert_eq!(b.align_self, AlignSelf::FlexEnd);
    assert_eq!(b.justify_content, JustifyContent::Center);
}

#[test]
fn test_box_position() {
    let b = Box::column()
        .position(Position::Absolute)
        .top(5)
        .left(10)
        .right(15)
        .bottom(20);
    
    assert_eq!(b.position, Position::Absolute);
    assert_eq!(b.top, Some(5));
    assert_eq!(b.left, Some(10));
    assert_eq!(b.right, Some(15));
    assert_eq!(b.bottom, Some(20));
}

#[test]
fn test_box_display_none() {
    let b = Box::column().display(Display::None);
    assert_eq!(b.display, Display::None);
}

#[test]
fn test_box_overflow() {
    let b = Box::column().overflow_x(Overflow::Hidden);
    assert_eq!(b.overflow_x, Overflow::Hidden);
}

#[test]
fn test_box_border_style() {
    let b = Box::column().border_style(BorderStyle::Round);
    assert_eq!(b.border_style, BorderStyle::Round);
    assert!(b.borders.top);
    assert!(b.borders.bottom);
}

#[test]
fn test_box_border_styles() {
    // Test all border styles
    for style in [
        BorderStyle::Single,
        BorderStyle::Double,
        BorderStyle::Round,
        BorderStyle::Bold,
        BorderStyle::Classic,
    ] {
        let b = Box::column().border_style(style);
        assert_eq!(b.border_style, style);
    }
}

#[test]
fn test_box_background_color() {
    let b = Box::column().background_color(Color::Blue);
    assert_eq!(b.background_color, Some(Color::Blue));
}

#[test]
fn test_box_children() {
    let b = Box::column()
        .child(Text::new("hello"))
        .child(Text::new("world"));
    assert_eq!(b.children.len(), 2);
}

#[test]
fn test_box_builder_pattern() {
    let b = Box::column()
        .padding(1)
        .border_style(BorderStyle::Round)
        .background_color(Color::Cyan)
        .child(Text::new("content"));
    
    assert_eq!(b.flex_direction, FlexDirection::Column);
    assert_eq!(b.padding_top, Some(1));
    assert_eq!(b.border_style, BorderStyle::Round);
    assert_eq!(b.background_color, Some(Color::Cyan));
    assert_eq!(b.children.len(), 1);
}

// =============================================================================
// Text tests
// =============================================================================

#[test]
fn test_text_new() {
    let t = Text::new("hello");
    assert_eq!(t.content, "hello");
    assert_eq!(t.color, Color::Default);
    assert!(!t.bold);
    assert!(!t.italic);
    assert!(!t.underline);
    assert!(!t.strikethrough);
    assert!(!t.dim_color);
    assert!(!t.inverse);
}

#[test]
fn test_text_styling() {
    let t = Text::new("hello")
        .color(Color::Red)
        .background_color(Color::Blue)
        .bold()
        .italic()
        .underline()
        .strikethrough()
        .dim()
        .inverse();
    
    assert_eq!(t.color, Color::Red);
    assert_eq!(t.background_color, Color::Blue);
    assert!(t.bold);
    assert!(t.italic);
    assert!(t.underline);
    assert!(t.strikethrough);
    assert!(t.dim_color);
    assert!(t.inverse);
}

#[test]
fn test_text_has_styling() {
    let plain = Text::new("hello");
    assert!(!plain.has_styling());
    
    let styled = Text::new("hello").bold().color(Color::Red);
    assert!(styled.has_styling());
}

#[test]
fn test_text_wrap() {
    let mut t = Text::new("hello");
    t.wrap = Wrap::Truncate;
    assert_eq!(t.wrap, Wrap::Truncate);
}

#[test]
fn test_text_all_wrap_modes() {
    for wrap in [Wrap::Wrap, Wrap::Hard, Wrap::Truncate, Wrap::TruncateMiddle] {
        let mut t = Text::new("test");
        t.wrap = wrap;
        assert_eq!(t.wrap, wrap);
    }
}

// =============================================================================
// Color tests
// =============================================================================

#[test]
fn test_color_default() {
    assert_eq!(Color::Default, Color::Default);
}

#[test]
fn test_color_named() {
    for color in [
        Color::Black,
        Color::Red,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Magenta,
        Color::Cyan,
        Color::White,
        Color::Gray,
    ] {
        let json = serde_json::to_string(&color).unwrap();
        let parsed: Color = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, color);
    }
}

#[test]
fn test_color_hex() {
    let c = Color::Hex("#ff00aa".to_string());
    let json = serde_json::to_string(&c).unwrap();
    assert!(json.contains("#ff00aa"));
    
    let parsed: Color = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, c);
}

#[test]
fn test_color_serde_roundtrip() {
    let colors = [
        Color::Default,
        Color::Black,
        Color::Red,
        Color::Green,
        Color::Hex("#123456".to_string()),
        Color::Hex("#abcdef".to_string()),
    ];
    
    for color in colors {
        let json = serde_json::to_string(&color).unwrap();
        let parsed: Color = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, color);
    }
}

// =============================================================================
// Other components
// =============================================================================

#[test]
fn test_newline() {
    let n = Newline::new();
    let _ = n; // Just ensure it can be constructed
}

#[test]
fn test_spacer() {
    let s = Spacer::new();
    let _ = s; // Just ensure it can be constructed
}

#[test]
fn test_static_new() {
    let s = Static::new();
    assert!(s.children.is_empty());
}

#[test]
fn test_static_children() {
    let s = Static::new()
        .child(Text::new("line 1"))
        .child(Text::new("line 2"));
    assert_eq!(s.children.len(), 2);
}

#[test]
fn test_transform_new() {
    let t = Transform::new(Text::new("hello"));
    assert_eq!(t.x, 0);
    assert_eq!(t.y, 0);
}

#[test]
fn test_transform_translate() {
    let t = Transform::new(Text::new("hello"))
        .translate(3, 5);
    assert_eq!(t.x, 3);
    assert_eq!(t.y, 5);
}

#[test]
fn test_transform_translate_x_y() {
    let t = Transform::new(Text::new("hello"))
        .translate_x(10)
        .translate_y(20);
    assert_eq!(t.x, 10);
    assert_eq!(t.y, 20);
}

// =============================================================================
// Style enums tests
// =============================================================================

#[test]
fn test_flex_direction_all() {
    for dir in [FlexDirection::Row, FlexDirection::Column, FlexDirection::RowReverse, FlexDirection::ColumnReverse] {
        let json = serde_json::to_string(&dir).unwrap();
        let parsed: FlexDirection = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, dir);
    }
}

#[test]
fn test_flex_wrap_all() {
    for wrap in [FlexWrap::NoWrap, FlexWrap::Wrap, FlexWrap::WrapReverse] {
        let json = serde_json::to_string(&wrap).unwrap();
        let parsed: FlexWrap = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, wrap);
    }
}

#[test]
fn test_align_items_all() {
    for align in [
        AlignItems::FlexStart,
        AlignItems::Center,
        AlignItems::FlexEnd,
        AlignItems::Stretch,
        AlignItems::Baseline,
    ] {
        let json = serde_json::to_string(&align).unwrap();
        let parsed: AlignItems = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, align);
    }
}

#[test]
fn test_align_self_all() {
    for align in [
        AlignSelf::Auto,
        AlignSelf::FlexStart,
        AlignSelf::Center,
        AlignSelf::FlexEnd,
        AlignSelf::Stretch,
        AlignSelf::Baseline,
    ] {
        let json = serde_json::to_string(&align).unwrap();
        let parsed: AlignSelf = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, align);
    }
}

#[test]
fn test_justify_content_all() {
    for justify in [
        JustifyContent::FlexStart,
        JustifyContent::Center,
        JustifyContent::FlexEnd,
        JustifyContent::SpaceBetween,
        JustifyContent::SpaceAround,
        JustifyContent::SpaceEvenly,
    ] {
        let json = serde_json::to_string(&justify).unwrap();
        let parsed: JustifyContent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, justify);
    }
}

#[test]
fn test_border_style_all() {
    for style in [
        BorderStyle::Single,
        BorderStyle::Double,
        BorderStyle::Round,
        BorderStyle::Bold,
        BorderStyle::Classic,
    ] {
        let json = serde_json::to_string(&style).unwrap();
        let parsed: BorderStyle = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, style);
    }
}

#[test]
fn test_display_all() {
    for display in [Display::Flex, Display::None] {
        let json = serde_json::to_string(&display).unwrap();
        let parsed: Display = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, display);
    }
}

#[test]
fn test_overflow_all() {
    for overflow in [Overflow::Visible, Overflow::Hidden] {
        let json = serde_json::to_string(&overflow).unwrap();
        let parsed: Overflow = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, overflow);
    }
}

#[test]
fn test_position_all() {
    for position in [Position::Relative, Position::Absolute] {
        let json = serde_json::to_string(&position).unwrap();
        let parsed: Position = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, position);
    }
}

#[test]
fn test_wrap_all() {
    for wrap in [Wrap::Wrap, Wrap::Hard, Wrap::Truncate, Wrap::TruncateMiddle] {
        let json = serde_json::to_string(&wrap).unwrap();
        let parsed: Wrap = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, wrap);
    }
}

// =============================================================================
// Borders tests
// =============================================================================

#[test]
fn test_borders_default() {
    let b = Borders::default();
    assert!(!b.top);
    assert!(!b.right);
    assert!(!b.bottom);
    assert!(!b.left);
}

#[test]
fn test_borders_all() {
    let b = Borders::ALL;
    assert!(b.top);
    assert!(b.right);
    assert!(b.bottom);
    assert!(b.left);
}

#[test]
fn test_borders_horizontal() {
    let b = Borders::HORIZONTAL;
    assert!(b.top);
    assert!(!b.right);
    assert!(b.bottom);
    assert!(!b.left);
}

#[test]
fn test_borders_vertical() {
    let b = Borders::VERTICAL;
    assert!(!b.top);
    assert!(b.right);
    assert!(!b.bottom);
    assert!(b.left);
}

#[test]
fn test_borders_serde() {
    for borders in [Borders::default(), Borders::ALL, Borders::HORIZONTAL, Borders::VERTICAL] {
        let json = serde_json::to_string(&borders).unwrap();
        let parsed: Borders = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.top, borders.top);
        assert_eq!(parsed.right, borders.right);
        assert_eq!(parsed.bottom, borders.bottom);
        assert_eq!(parsed.left, borders.left);
    }
}
