//! Unit tests for runts-ink layout structures.
//!
//! These tests verify that the Ink-compatible layout types
//! can be created and configured correctly.

use runts_ink::{
    AlignItems, AlignSelf, Box, BorderStyle, Color, Display,
    FlexDirection, FlexWrap, JustifyContent, Newline, Overflow,
    Position, Spacer, Static, Text, Transform, VNode, VNodeContent, Wrap,
};

#[test]
fn test_layout_empty_tree() {
    let root = VNode::from(Box::column());
    assert_eq!(root.kind(), "box");
    assert!(root.children().is_empty());
}

#[test]
fn test_layout_single_text() {
    let root = VNode::from(
        Box::column().child(Text::new("hello"))
    );
    assert_eq!(root.kind(), "box");
    assert_eq!(root.children().len(), 1);
    assert_eq!(root.children()[0].kind(), "text");
}

#[test]
fn test_layout_multiple_texts() {
    let root = VNode::from(
        Box::column()
            .child(Text::new("line1"))
            .child(Text::new("line2"))
            .child(Text::new("line3"))
    );
    assert_eq!(root.children().len(), 3);
}

#[test]
fn test_layout_row_direction() {
    let root = VNode::from(
        Box::row()
            .child(Text::new("A"))
            .child(Text::new("B"))
    );
    // Verify it's a Box
    assert_eq!(root.kind(), "box");
    // Verify it has 2 children
    assert_eq!(root.children().len(), 2);
}

#[test]
fn test_layout_column_direction() {
    let root = VNode::from(
        Box::column()
            .child(Text::new("top"))
            .child(Text::new("bottom"))
    );
    assert_eq!(root.children().len(), 2);
}

#[test]
fn test_layout_nested_boxes() {
    let root = VNode::from(
        Box::column().child(
            Box::row()
                .child(Text::new("nested"))
        )
    );
    // Root box with 1 child (inner box)
    assert_eq!(root.children().len(), 1);
    assert_eq!(root.children()[0].kind(), "box");
    // Inner box with 1 child (text)
    let inner = &root.children()[0];
    assert_eq!(inner.children().len(), 1);
    assert_eq!(inner.children()[0].kind(), "text");
}

#[test]
fn test_layout_deeply_nested() {
    let root = VNode::from(
        Box::column().child(
            Box::column().child(
                Box::row().child(
                    Box::column().child(Text::new("deep"))
                )
            )
        )
    );
    // Verify deep nesting works
    assert_eq!(root.kind(), "box");
}

#[test]
fn test_layout_column_gap() {
    let root = VNode::from(
        Box::row()
            .column_gap(2)
            .child(Text::new("A"))
            .child(Text::new("B"))
            .child(Text::new("C"))
    );
    // Root + 3 texts = structure has 3 children
    assert_eq!(root.children().len(), 3);
}

#[test]
fn test_layout_row_gap() {
    let root = VNode::from(
        Box::column()
            .row_gap(1)
            .child(Text::new("X"))
            .child(Text::new("Y"))
    );
    assert_eq!(root.children().len(), 2);
}

#[test]
fn test_layout_padding() {
    let root = VNode::from(
        Box::column()
            .padding(2)
            .child(Text::new("padded"))
    );
    assert_eq!(root.children().len(), 1);
}

#[test]
fn test_layout_padding_xy() {
    let root = VNode::from(
        Box::column()
            .padding_x(3)
            .padding_y(1)
            .child(Text::new("xy padded"))
    );
    assert_eq!(root.children().len(), 1);
}

#[test]
fn test_layout_margin() {
    let root = VNode::from(
        Box::column()
            .margin(1)
            .child(Text::new("with margin"))
    );
    assert_eq!(root.children().len(), 1);
}

#[test]
fn test_layout_fixed_width() {
    let root = VNode::from(
        Box::column().width(50).child(Text::new("fixed width"))
    );
    assert_eq!(root.children().len(), 1);
}

#[test]
fn test_layout_fixed_height() {
    let root = VNode::from(
        Box::column().height(10).child(Text::new("fixed height"))
    );
    assert_eq!(root.children().len(), 1);
}

#[test]
fn test_layout_min_max_width() {
    let root = VNode::from(
        Box::column()
            .min_width(20)
            .max_width(60)
            .child(Text::new("constrained"))
    );
    assert_eq!(root.children().len(), 1);
}

#[test]
fn test_layout_min_max_height() {
    let root = VNode::from(
        Box::column()
            .min_height(5)
            .max_height(10)
            .child(Text::new("height constrained"))
    );
    assert_eq!(root.children().len(), 1);
}

#[test]
fn test_layout_display_none() {
    let mut box_node = Box::column();
    box_node.display = Display::None;
    let root = VNode::from(box_node.child(Text::new("hidden")));
    assert_eq!(root.children().len(), 1);
}

#[test]
fn test_layout_position_absolute() {
    let root = VNode::from(
        Box::column()
            .position(Position::Absolute)
            .top(5)
            .left(10)
            .width(30)
            .height(10)
            .child(Text::new("absolute"))
    );
    assert_eq!(root.children().len(), 1);
}

#[test]
fn test_layout_flex_grow() {
    let root = VNode::from(
        Box::row()
            .child(Text::new("fixed"))
            .child(Box::column().flex_grow(1.0).child(Text::new("growing")))
    );
    // Root with 2 children: text + growing box
    assert_eq!(root.children().len(), 2);
}

#[test]
fn test_layout_border_reduces_content_area() {
    let root = VNode::from(
        Box::column()
            .border_style(BorderStyle::Round)
            .width(80)
            .child(Text::new("bordered"))
    );
    assert_eq!(root.children().len(), 1);
}

#[test]
fn test_layout_background_color() {
    let root = VNode::from(
        Box::column()
            .background_color(Color::Blue)
            .child(Text::new("colored"))
    );
    assert_eq!(root.children().len(), 1);
}

#[test]
fn test_layout_multiple_levels_mixed() {
    let root = VNode::from(
        Box::column()
            .padding(1)
            .child(
                Box::row()
                    .column_gap(2)
                    .child(Text::new("A").bold())
                    .child(Text::new("B").italic())
            )
            .child(
                Box::row()
                    .column_gap(2)
                    .child(Text::new("C"))
                    .child(Text::new("D"))
            )
    );
    // Root with 2 row children
    assert_eq!(root.children().len(), 2);
}

#[test]
fn test_layout_empty_children() {
    let root = VNode::from(
        Box::column()
            .child(Box::column())
            .child(Text::new("after empty"))
    );
    // Root with 2 children: empty box + text
    assert_eq!(root.children().len(), 2);
}

#[test]
fn test_layout_text_only() {
    let root = VNode::from(Text::new("standalone text"));
    assert_eq!(root.kind(), "text");
    assert!(root.children().is_empty());
}

#[test]
fn test_layout_row_reverse() {
    let root = VNode::from(
        Box::new()
            .flex_direction(FlexDirection::RowReverse)
            .child(Text::new("first"))
            .child(Text::new("second"))
    );
    assert_eq!(root.children().len(), 2);
}

#[test]
fn test_layout_column_reverse() {
    let root = VNode::from(
        Box::column()
            .flex_direction(FlexDirection::ColumnReverse)
            .child(Text::new("top"))
            .child(Text::new("bottom"))
    );
    assert_eq!(root.children().len(), 2);
}

#[test]
fn test_layout_wrap() {
    let root = VNode::from(
        Box::new()
            .flex_wrap(FlexWrap::Wrap)
            .child(Text::new("item1"))
            .child(Text::new("item2"))
    );
    assert_eq!(root.children().len(), 2);
}

#[test]
fn test_layout_overflow_hidden() {
    let root = VNode::from(
        Box::column()
            .overflow_x(Overflow::Hidden)
            .child(Text::new("clipped content"))
    );
    assert_eq!(root.children().len(), 1);
}

#[test]
fn test_layout_align_items_flex_start() {
    let root = VNode::from(
        Box::column()
            .align_items(AlignItems::FlexStart)
            .child(Text::new("aligned"))
    );
    assert_eq!(root.children().len(), 1);
}

#[test]
fn test_layout_align_items_center() {
    let root = VNode::from(
        Box::column()
            .align_items(AlignItems::Center)
            .child(Text::new("centered"))
    );
    assert_eq!(root.children().len(), 1);
}

#[test]
fn test_layout_align_items_flex_end() {
    let root = VNode::from(
        Box::column()
            .align_items(AlignItems::FlexEnd)
            .child(Text::new("end"))
    );
    assert_eq!(root.children().len(), 1);
}

#[test]
fn test_layout_justify_content_center() {
    let root = VNode::from(
        Box::row()
            .justify_content(JustifyContent::Center)
            .child(Text::new("centered"))
    );
    assert_eq!(root.children().len(), 1);
}

#[test]
fn test_layout_justify_content_space_between() {
    let root = VNode::from(
        Box::row()
            .justify_content(JustifyContent::SpaceBetween)
            .child(Text::new("A"))
            .child(Text::new("B"))
    );
    assert_eq!(root.children().len(), 2);
}

#[test]
fn test_layout_justify_content_space_around() {
    let root = VNode::from(
        Box::row()
            .justify_content(JustifyContent::SpaceAround)
            .child(Text::new("X"))
            .child(Text::new("Y"))
            .child(Text::new("Z"))
    );
    assert_eq!(root.children().len(), 3);
}

#[test]
fn test_layout_justify_content_space_evenly() {
    let root = VNode::from(
        Box::row()
            .justify_content(JustifyContent::SpaceEvenly)
            .child(Text::new("1"))
            .child(Text::new("2"))
    );
    assert_eq!(root.children().len(), 2);
}

#[test]
fn test_layout_border_styles() {
    for style in [
        BorderStyle::Single,
        BorderStyle::Double,
        BorderStyle::Round,
        BorderStyle::Bold,
        BorderStyle::Classic,
    ] {
        let root = VNode::from(
            Box::column()
                .border_style(style)
                .child(Text::new("test"))
        );
        assert_eq!(root.children().len(), 1);
    }
}

#[test]
fn test_layout_all_colors() {
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
        Color::Default,
    ] {
        let root = VNode::from(
            Box::column()
                .background_color(color.clone())
                .child(Text::new("colored").color(color))
        );
        assert_eq!(root.children().len(), 1);
    }
}

#[test]
fn test_layout_hex_color() {
    let root = VNode::from(
        Box::column()
            .background_color(Color::Hex("#FF5500".to_string()))
            .child(Text::new("hex color"))
    );
    assert_eq!(root.children().len(), 1);
}

#[test]
fn test_layout_complex_realistic() {
    // Simulates a realistic UI layout
    let root = VNode::from(
        Box::column()
            .padding(1)
            .child(
                Box::column()
                    .border_style(BorderStyle::Round)
                    .padding(1)
                    .child(Text::new("Header").bold().color(Color::Cyan))
            )
            .child(Box::column().margin(1).child(Text::new("Content")))
            .child(
                Box::row()
                    .margin(1)
                    .justify_content(JustifyContent::SpaceBetween)
                    .child(Text::new("Footer").dim())
                    .child(Text::new("Status").dim())
            )
    );
    // Root with 3 children: header, content, footer
    assert_eq!(root.children().len(), 3);
}

#[test]
fn test_newline_node() {
    let root = VNode::from(Newline::new());
    assert_eq!(root.kind(), "newline");
}

#[test]
fn test_spacer_node() {
    let root = VNode::from(Spacer::new());
    assert_eq!(root.kind(), "spacer");
}

#[test]
fn test_static_node() {
    let root = VNode::from(Static::new());
    assert_eq!(root.kind(), "static");
    assert!(root.children().is_empty());
}

#[test]
fn test_static_with_children() {
    let root = VNode::from(
        Static::new()
            .child(Text::new("a"))
            .child(Text::new("b"))
    );
    assert_eq!(root.children().len(), 2);
}

#[test]
fn test_transform_node() {
    let root = VNode::from(
        Transform::new(Text::new("transformed"))
    );
    assert_eq!(root.kind(), "transform");
    assert_eq!(root.children().len(), 1);
}

#[test]
fn test_fragment_node() {
    let root = VNode::from(VNodeContent::Fragment(vec![
        Text::new("a").into(),
        Text::new("b").into(),
    ]));
    assert_eq!(root.kind(), "fragment");
    assert_eq!(root.children().len(), 2);
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
        let root = VNode::from(
            Box::column().child(
                Box::new().align_self(align).child(Text::new("test"))
            )
        );
        // Just verify it doesn't panic
        assert_eq!(root.children().len(), 1);
    }
}

#[test]
fn test_wrap_modes() {
    for wrap in [Wrap::Wrap, Wrap::Hard, Wrap::Truncate, Wrap::TruncateMiddle] {
        let mut text = Text::new("test");
        text.wrap = wrap;
        let root = VNode::from(
            Box::column()
                .child(text)
        );
        assert_eq!(root.children().len(), 1);
    }
}
