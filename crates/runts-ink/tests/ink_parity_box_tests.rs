//! Parity tests for Ink Box component.
//!
//! Tests Box rendering, flexbox properties, borders, backgrounds,
//! and all layout-related features.

use runts_ink::{
    AlignItems, AlignSelf, Box as InkBox, BorderStyle, Color, Display,
    FlexDirection, FlexWrap, JustifyContent, Overflow, Position,
    render_to_string, RenderOptions, Text as InkText, VNode,
};

#[cfg(test)]
mod box_basic_tests {
    use super::*;

    #[test]
    fn test_box_empty_render() {
        let root = VNode::from(InkBox::column());
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.is_empty() || s.lines().all(|l| l.trim().is_empty()));
    }

    #[test]
    fn test_box_with_single_text() {
        let root = VNode::from(InkBox::column().child(InkText::new("Hello, World!")));
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Hello, World!"));
    }

    #[test]
    fn test_box_with_multiple_text_children() {
        let root = VNode::from(
            InkBox::column()
                .child(InkText::new("Line 1"))
                .child(InkText::new("Line 2"))
                .child(InkText::new("Line 3")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Line 1"));
        assert!(s.contains("Line 2"));
        assert!(s.contains("Line 3"));
    }

    #[test]
    fn test_box_row_flex_direction() {
        let root = VNode::from(
            InkBox::row().child(InkText::new("A")).child(InkText::new("B")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("A"));
        assert!(s.contains("B"));
    }

    #[test]
    fn test_box_column_flex_direction() {
        let root = VNode::from(
            InkBox::column().child(InkText::new("Top")).child(InkText::new("Bottom")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Top"));
        assert!(s.contains("Bottom"));
    }
}

#[cfg(test)]
mod box_padding_tests {
    use super::*;

    #[test]
    fn test_box_padding() {
        let root = VNode::from(
            InkBox::column().padding(2).child(InkText::new("Padded")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Padded"));
    }

    #[test]
    fn test_box_padding_x() {
        let root = VNode::from(
            InkBox::column().padding_x(3).child(InkText::new("X Padded")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("X Padded"));
    }

    #[test]
    fn test_box_padding_y() {
        let root = VNode::from(
            InkBox::column().padding_y(1).child(InkText::new("Y Padded")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Y Padded"));
    }

    #[test]
    fn test_box_margin() {
        let root = VNode::from(
            InkBox::column().margin(1).child(InkText::new("With Margin")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("With Margin"));
    }
}

#[cfg(test)]
mod box_size_tests {
    use super::*;

    #[test]
    fn test_box_width_height() {
        let root = VNode::from(
            InkBox::column()
                .width(80)
                .height(24)
                .child(InkText::new("Fixed Size")),
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
                .child(InkText::new("Constrained")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Constrained"));
    }

    #[test]
    fn test_box_flex_grow() {
        let root = VNode::from(
            InkBox::row().flex_grow(1.0).child(InkText::new("Growing")),
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
                .child(InkText::new("B")),
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
                .child(InkText::new("Y")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("X"));
        assert!(s.contains("Y"));
    }
}

#[cfg(test)]
mod box_alignment_tests {
    use super::*;

    #[test]
    fn test_box_align_items_center() {
        let root = VNode::from(
            InkBox::column()
                .align_items(AlignItems::Center)
                .child(InkText::new("Centered")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Centered"));
    }

    #[test]
    fn test_box_align_items_flex_end() {
        let root = VNode::from(
            InkBox::column()
                .align_items(AlignItems::FlexEnd)
                .child(InkText::new("End")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("End"));
    }

    #[test]
    fn test_box_justify_content_center() {
        let root = VNode::from(
            InkBox::row()
                .justify_content(JustifyContent::Center)
                .child(InkText::new("Justify")),
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
                .child(InkText::new("B")),
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
                .child(InkText::new("Y")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("X"));
        assert!(s.contains("Y"));
    }
}

#[cfg(test)]
mod box_display_position_tests {
    use super::*;

    #[test]
    fn test_box_display_none() {
        let root = VNode::from(
            InkBox::column()
                .display(Display::None)
                .child(InkText::new("Hidden")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(!s.contains("Hidden") || s.trim().is_empty());
    }

    #[test]
    fn test_box_position_absolute() {
        let root = VNode::from(
            InkBox::column()
                .position(Position::Absolute)
                .top(5)
                .left(10)
                .child(InkText::new("Absolute")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Absolute"));
    }

    #[test]
    fn test_box_overflow_hidden() {
        let mut b = InkBox::column();
        b.overflow_x = Overflow::Hidden;
        b.overflow_y = Overflow::Hidden;
        let root = VNode::from(b.child(InkText::new("Clipped")));
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Clipped"));
    }
}

#[cfg(test)]
mod box_border_tests {
    use super::*;

    #[test]
    fn test_box_border_single() {
        let root = VNode::from(
            InkBox::column()
                .border_style(BorderStyle::Single)
                .child(InkText::new("Bordered")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Bordered"));
    }

    #[test]
    fn test_box_border_double() {
        let root = VNode::from(
            InkBox::column()
                .border_style(BorderStyle::Double)
                .child(InkText::new("Double Bordered")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Double Bordered"));
    }

    #[test]
    fn test_box_border_round() {
        let root = VNode::from(
            InkBox::column()
                .border_style(BorderStyle::Round)
                .child(InkText::new("Round Bordered")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Round Bordered"));
    }

    #[test]
    fn test_box_border_bold() {
        let root = VNode::from(
            InkBox::column()
                .border_style(BorderStyle::Bold)
                .child(InkText::new("Bold Bordered")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Bold Bordered"));
    }

    #[test]
    fn test_box_border_classic() {
        let root = VNode::from(
            InkBox::column()
                .border_style(BorderStyle::Classic)
                .child(InkText::new("Classic Bordered")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Classic Bordered"));
    }

    #[test]
    fn test_box_background_color() {
        let root = VNode::from(
            InkBox::column()
                .background_color(Color::Blue)
                .child(InkText::new("Colored Background")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Colored Background"));
    }

    #[test]
    fn test_box_border_color() {
        let mut b = InkBox::column();
        b.border_style = BorderStyle::Round;
        b.border_color = Some(Color::Cyan);
        let root = VNode::from(b.child(InkText::new("Colored Border")));
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Colored Border"));
    }
}

#[cfg(test)]
mod box_flex_tests {
    use super::*;

    #[test]
    fn test_box_flex_direction_row() {
        let root = VNode::from(
            InkBox::new()
                .flex_direction(FlexDirection::Row)
                .child(InkText::new("Row")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Row"));
    }

    #[test]
    fn test_box_flex_direction_column() {
        let root = VNode::from(
            InkBox::new()
                .flex_direction(FlexDirection::Column)
                .child(InkText::new("Column")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Column"));
    }

    #[test]
    fn test_box_flex_wrap() {
        let root = VNode::from(
            InkBox::new().flex_wrap(FlexWrap::Wrap).child(InkText::new("Wrapped")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Wrapped"));
    }

    #[test]
    fn test_box_flex_wrap_reverse() {
        let root = VNode::from(
            InkBox::new()
                .flex_wrap(FlexWrap::WrapReverse)
                .child(InkText::new("Wrap Reverse")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Wrap Reverse"));
    }

    #[test]
    fn test_box_align_self() {
        let root = VNode::from(
            InkBox::column().child(
                InkBox::new()
                    .align_self(AlignSelf::FlexEnd)
                    .child(InkText::new("Align Self")),
            ),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Align Self"));
    }
}

#[cfg(test)]
mod box_builder_tests {
    use super::*;

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
}
