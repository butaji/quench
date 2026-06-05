//! Parity tests for serialization of Ink types.
//!
//! Tests JSON serialization and deserialization roundtrips
//! for all Ink-compatible types.

use runts_ink::{
    AlignItems, AlignSelf, Box as InkBox, BorderStyle, Borders, Color, Display,
    FlexDirection, FlexWrap, JustifyContent, Overflow, Position, Text as InkText,
    Transform, VNode, Wrap,
};

#[cfg(test)]
mod box_serde_tests {
    use super::*;

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
}

#[cfg(test)]
mod text_serde_tests {
    use super::*;

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
}

#[cfg(test)]
mod color_serde_tests {
    use super::*;

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
}

#[cfg(test)]
mod borders_serde_tests {
    use super::*;

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
}

#[cfg(test)]
mod flex_serde_tests {
    use super::*;

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
        let wraps = [FlexWrap::NoWrap, FlexWrap::Wrap, FlexWrap::WrapReverse];

        for wrap in wraps {
            let json = serde_json::to_string(&wrap).unwrap();
            let parsed: FlexWrap = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, wrap);
        }
    }
}

#[cfg(test)]
mod alignment_serde_tests {
    use super::*;

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
}

#[cfg(test)]
mod style_serde_tests {
    use super::*;

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
}
