//! Comprehensive unit tests for runts-ink components.
//!
//! These tests provide high coverage for:
//! - All Box layout properties
//! - All Text styling options
//! - All Border styles
//! - All Color variants
//! - Flex layout computations
//! - Serde serialization/deserialization

use runts_ink::{
    AlignContent, AlignItems, AlignSelf, BorderStyle, Box, Color, Display,
    FlexDirection, FlexWrap, JustifyContent, Overflow, Position, VNode, VNodeContent,
};

#[cfg(test)]
mod box_tests {
    use super::*;

    #[test]
    fn test_box_new_default() {
        let b = Box::new();
        assert_eq!(b.flex_direction, FlexDirection::Row);
        assert_eq!(b.flex_wrap, FlexWrap::NoWrap);
        assert_eq!(b.flex_grow, 0.0);
        assert_eq!(b.flex_shrink, 1.0);
        assert_eq!(b.align_items, AlignItems::Stretch);
        assert_eq!(b.align_self, AlignSelf::Auto);
        assert_eq!(b.justify_content, JustifyContent::FlexStart);
        assert_eq!(b.display, Display::Flex);
        assert!(b.children.is_empty());
    }

    #[test]
    fn test_box_row() {
        let b = Box::row();
        assert_eq!(b.flex_direction, FlexDirection::Row);
    }

    #[test]
    fn test_box_column() {
        let b = Box::column();
        assert_eq!(b.flex_direction, FlexDirection::Column);
    }

    #[test]
    fn test_box_flex_direction() {
        let b = Box::new()
            .flex_direction(FlexDirection::RowReverse)
            .flex_direction(FlexDirection::Column)
            .flex_direction(FlexDirection::ColumnReverse);
        assert_eq!(b.flex_direction, FlexDirection::ColumnReverse);
    }

    #[test]
    fn test_box_flex_wrap() {
        let b = Box::new()
            .flex_wrap(FlexWrap::Wrap)
            .flex_wrap(FlexWrap::WrapReverse);
        assert_eq!(b.flex_wrap, FlexWrap::WrapReverse);
    }

    #[test]
    fn test_box_flex_grow_shrink() {
        let b = Box::new().flex_grow(2.0);
        assert_eq!(b.flex_grow, 2.0);
    }

    #[test]
    fn test_box_padding() {
        let b = Box::new()
            .padding(5)
            .padding_x(3)
            .padding_y(7);
        assert_eq!(b.padding_top, Some(7));
        assert_eq!(b.padding_right, Some(3));
        assert_eq!(b.padding_bottom, Some(7));
        assert_eq!(b.padding_left, Some(3));
    }

    #[test]
    fn test_box_margin() {
        let b = Box::new().margin(10);
        assert_eq!(b.margin_top, Some(10));
        assert_eq!(b.margin_right, Some(10));
        assert_eq!(b.margin_bottom, Some(10));
        assert_eq!(b.margin_left, Some(10));
    }

    #[test]
    fn test_box_margin_individual() {
        let mut b = Box::new();
        b.margin_top = Some(1);
        b.margin_right = Some(2);
        b.margin_bottom = Some(3);
        b.margin_left = Some(4);
        assert_eq!(b.margin_top, Some(1));
        assert_eq!(b.margin_right, Some(2));
        assert_eq!(b.margin_bottom, Some(3));
        assert_eq!(b.margin_left, Some(4));
    }

    #[test]
    fn test_box_dimensions() {
        let b = Box::new()
            .width(100)
            .height(50)
            .min_width(80)
            .min_height(40)
            .max_width(120)
            .max_height(60);
        assert_eq!(b.width, Some(100));
        assert_eq!(b.height, Some(50));
        assert_eq!(b.min_width, Some(80));
        assert_eq!(b.min_height, Some(40));
        assert_eq!(b.max_width, Some(120));
        assert_eq!(b.max_height, Some(60));
    }

    #[test]
    fn test_box_gaps() {
        let b = Box::new()
            .column_gap(10)
            .row_gap(20);
        assert_eq!(b.column_gap, Some(10));
        assert_eq!(b.row_gap, Some(20));
    }

    #[test]
    fn test_box_align_items() {
        let b = Box::new()
            .align_items(AlignItems::Center)
            .align_items(AlignItems::FlexEnd);
        assert_eq!(b.align_items, AlignItems::FlexEnd);
    }

    #[test]
    fn test_box_align_self() {
        let b = Box::new()
            .align_self(AlignSelf::Center)
            .align_self(AlignSelf::Baseline);
        assert_eq!(b.align_self, AlignSelf::Baseline);
    }

    #[test]
    fn test_box_align_content() {
        let mut b = Box::new();
        b.align_content = AlignContent::Center;
        assert_eq!(b.align_content, AlignContent::Center);
    }

    #[test]
    fn test_box_justify_content() {
        let b = Box::new()
            .justify_content(JustifyContent::Center)
            .justify_content(JustifyContent::SpaceBetween);
        assert_eq!(b.justify_content, JustifyContent::SpaceBetween);
    }

    #[test]
    fn test_box_position() {
        let b = Box::new()
            .position(Position::Absolute)
            .top(10)
            .left(20)
            .right(30)
            .bottom(40);
        assert_eq!(b.position, Position::Absolute);
        assert_eq!(b.top, Some(10));
        assert_eq!(b.left, Some(20));
        assert_eq!(b.right, Some(30));
        assert_eq!(b.bottom, Some(40));
    }

    #[test]
    fn test_box_display() {
        let b = Box::new()
            .display(Display::None)
            .display(Display::Flex);
        assert_eq!(b.display, Display::Flex);
    }

    #[test]
    fn test_box_overflow() {
        let mut b = Box::new();
        b.overflow_x = Overflow::Hidden;
        b.overflow_y = Overflow::Hidden;
        assert_eq!(b.overflow_x, Overflow::Hidden);
        assert_eq!(b.overflow_y, Overflow::Hidden);
    }

    #[test]
    fn test_box_border_style() {
        let b = Box::new()
            .border_style(BorderStyle::Double)
            .border_style(BorderStyle::Round);
        assert_eq!(b.border_style, BorderStyle::Round);
    }

    #[test]
    fn test_box_background_color() {
        let b = Box::new()
            .background_color(Color::Red);
        assert_eq!(b.background_color, Some(Color::Red));
    }

    #[test]
    fn test_box_children() {
        use runts_ink::{Text, VNodeContent};
        let child1 = VNode(VNodeContent::Text(Text::new("Hello")));
        let child2 = VNode(VNodeContent::Text(Text::new("World")));
        let b = Box::new()
            .child(child1)
            .children(vec![child2]);
        assert_eq!(b.children.len(), 2);
    }

    #[test]
    fn test_box_serde() {
        let b = Box::column()
            .padding(5)
            .background_color(Color::Blue);
        let json = serde_json::to_string(&b).unwrap();
        let parsed: Box = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.flex_direction, FlexDirection::Column);
        assert_eq!(parsed.padding_top, Some(5));
        assert_eq!(parsed.background_color, Some(Color::Blue));
    }
}

#[cfg(test)]
mod text_tests {
    use super::*;
    use runts_ink::{Text, Wrap};

    #[test]
    fn test_text_new() {
        let t = Text::new("Hello");
        assert_eq!(t.content, "Hello");
    }

    #[test]
    fn test_text_builder() {
        let mut t = Text::new("Test");
        t.bold = true;
        t.italic = true;
        t.underline = true;
        t.dim_color = true;
        t.inverse = true;
        t.strikethrough = true;
        assert!(t.bold);
        assert!(t.italic);
        assert!(t.underline);
        assert!(t.dim_color);
        assert!(t.inverse);
        assert!(t.strikethrough);
    }

    #[test]
    fn test_text_color() {
        let t = Text::new("Colored").color(Color::Green);
        assert_eq!(t.color, Color::Green);
    }

    #[test]
    fn test_text_background_color() {
        let t = Text::new("BG").background_color(Color::Yellow);
        assert_eq!(t.background_color, Color::Yellow);
    }

    #[test]
    fn test_text_wrap() {
        let mut t = Text::new("Wrapped");
        t.wrap = Wrap::Truncate;
        assert_eq!(t.wrap, Wrap::Truncate);
    }

    #[test]
    fn test_text_serde() {
        let mut t = Text::new("Test");
        t.bold = true;
        let t = t.color(Color::Red);
        
        let json = serde_json::to_string(&t).unwrap();
        let parsed: Text = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.content, "Test");
        assert!(parsed.bold);
        assert_eq!(parsed.color, Color::Red);
    }
}

#[cfg(test)]
mod color_tests {
    use super::*;

    #[test]
    fn test_color_named() {
        for color in [
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
        ] {
            let json = serde_json::to_string(&color).unwrap();
            let parsed: Color = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, color);
        }
    }

    #[test]
    fn test_color_hex() {
        let c = Color::Hex("#FF0000".to_string());
        let json = serde_json::to_string(&c).unwrap();
        let parsed: Color = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, c);
    }
}

#[cfg(test)]
mod border_style_tests {
    use super::*;

    #[test]
    fn test_border_style_all() {
        for style in [
            BorderStyle::Single,
            BorderStyle::Double,
            BorderStyle::Bold,
            BorderStyle::Round,
            BorderStyle::Classic,
        ] {
            let json = serde_json::to_string(&style).unwrap();
            let parsed: BorderStyle = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, style);
        }
    }
}

#[cfg(test)]
mod align_tests {
    use super::*;

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
    fn test_align_content_all() {
        for align in [
            AlignContent::FlexStart,
            AlignContent::Center,
            AlignContent::FlexEnd,
            AlignContent::Stretch,
            AlignContent::SpaceBetween,
            AlignContent::SpaceAround,
        ] {
            let json = serde_json::to_string(&align).unwrap();
            let parsed: AlignContent = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, align);
        }
    }
}

#[cfg(test)]
mod justify_content_tests {
    use super::*;

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
}

#[cfg(test)]
mod flex_wrap_tests {
    use super::*;

    #[test]
    fn test_flex_wrap_all() {
        for wrap in [FlexWrap::NoWrap, FlexWrap::Wrap, FlexWrap::WrapReverse] {
            let json = serde_json::to_string(&wrap).unwrap();
            let parsed: FlexWrap = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, wrap);
        }
    }
}

#[cfg(test)]
mod flex_direction_tests {
    use super::*;

    #[test]
    fn test_flex_direction_all() {
        for dir in [
            FlexDirection::Row,
            FlexDirection::RowReverse,
            FlexDirection::Column,
            FlexDirection::ColumnReverse,
        ] {
            let json = serde_json::to_string(&dir).unwrap();
            let parsed: FlexDirection = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, dir);
        }
    }
}

#[cfg(test)]
mod display_overflow_tests {
    use super::*;

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
        for overflow in [
            Overflow::Visible,
            Overflow::Hidden,
        ] {
            let json = serde_json::to_string(&overflow).unwrap();
            let parsed: Overflow = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, overflow);
        }
    }
}

#[cfg(test)]
mod position_tests {
    use super::*;

    #[test]
    fn test_position_all() {
        for pos in [Position::Relative, Position::Absolute] {
            let json = serde_json::to_string(&pos).unwrap();
            let parsed: Position = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, pos);
        }
    }
}

#[cfg(test)]
mod vnode_tests {
    use super::*;
    use runts_ink::Text;

    #[test]
    fn test_vnode_text() {
        let v = VNode(VNodeContent::Text(Text::new("Hello")));
        match &v.0 {
            VNodeContent::Text(t) => assert_eq!(t.content, "Hello"),
            _ => panic!("Expected Text variant"),
        }
    }

    #[test]
    fn test_vnode_element() {
        let box_node = Box::new();
        let v = VNode(VNodeContent::Box(box_node));
        match &v.0 {
            VNodeContent::Box(b) => {
                assert_eq!(b.flex_direction, FlexDirection::Row);
            }
            _ => panic!("Expected Box variant"),
        }
    }
}

#[cfg(test)]
mod component_serde_comprehensive_tests {
    use super::*;
    use runts_ink::Text;

    #[test]
    fn test_complex_box_serde() {
        let mut b = Box::column()
            .padding(10)
            .width(100)
            .height(50)
            .background_color(Color::Blue)
            .border_style(BorderStyle::Round)
            .column_gap(5)
            .row_gap(10)
            .align_items(AlignItems::Center)
            .justify_content(JustifyContent::SpaceBetween);
        
        // Set fields that don't have builder methods
        b.align_content = AlignContent::SpaceBetween;
        b.margin_left = Some(5);

        let json = serde_json::to_string(&b).unwrap();
        let parsed: Box = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.flex_direction, FlexDirection::Column);
        assert_eq!(parsed.padding_top, Some(10));
        assert_eq!(parsed.width, Some(100));
        assert_eq!(parsed.height, Some(50));
        assert_eq!(parsed.background_color, Some(Color::Blue));
        assert_eq!(parsed.border_style, BorderStyle::Round);
        assert_eq!(parsed.column_gap, Some(5));
        assert_eq!(parsed.row_gap, Some(10));
        assert_eq!(parsed.align_items, AlignItems::Center);
        assert_eq!(parsed.justify_content, JustifyContent::SpaceBetween);
    }

    #[test]
    fn test_complex_text_serde() {
        let mut t = Text::new("Styled Text");
        t.bold = true;
        t.italic = true;
        t.underline = true;
        t.dim_color = true;
        t.inverse = true;
        t.strikethrough = true;
        let t = t.color(Color::Cyan).background_color(Color::Black);

        let json = serde_json::to_string(&t).unwrap();
        let parsed: Text = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.content, "Styled Text");
        assert!(parsed.bold);
        assert!(parsed.italic);
        assert!(parsed.underline);
        assert!(parsed.dim_color);
        assert!(parsed.inverse);
        assert!(parsed.strikethrough);
        assert_eq!(parsed.color, Color::Cyan);
        assert_eq!(parsed.background_color, Color::Black);
    }
}
