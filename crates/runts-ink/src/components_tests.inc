#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Box, Color, FlexDirection, Static, Text, Transform};

    #[test]
    fn box_default_is_row_with_no_children() {
        let b = Box::default();
        assert_eq!(b.flex_direction, FlexDirection::Row);
        assert!(b.children.is_empty());
    }

    #[test]
    fn box_column_helper_sets_flex_direction() {
        let b = Box::column();
        assert_eq!(b.flex_direction, FlexDirection::Column);
    }

    #[test]
    fn box_padding_sets_all_sides() {
        let b = Box::column().padding(2);
        assert_eq!(b.padding_top, Some(2));
        assert_eq!(b.padding_right, Some(2));
        assert_eq!(b.padding_bottom, Some(2));
        assert_eq!(b.padding_left, Some(2));
    }

    #[test]
    fn box_padding_x_y_shorthands() {
        // paddingX sets left and right but leaves
        // top/bottom alone.
        let b = Box::column().padding_x(3);
        assert_eq!(b.padding_left, Some(3));
        assert_eq!(b.padding_right, Some(3));
        assert_eq!(b.padding_top, None);
        assert_eq!(b.padding_bottom, None);
        // paddingY sets top and bottom but leaves
        // left/right alone.
        let b = Box::column().padding_y(1);
        assert_eq!(b.padding_top, Some(1));
        assert_eq!(b.padding_bottom, Some(1));
        assert_eq!(b.padding_left, None);
        assert_eq!(b.padding_right, None);
        // Combined: paddingX + paddingY is equivalent
        // to padding for a 4-side box, but a user
        // can also use padding_x and padding_y
        // independently.
        let b = Box::column().padding_x(2).padding_y(1);
        assert_eq!(b.padding_left, Some(2));
        assert_eq!(b.padding_right, Some(2));
        assert_eq!(b.padding_top, Some(1));
        assert_eq!(b.padding_bottom, Some(1));
    }

    #[test]
    fn box_margin_sets_all_sides() {
        let b = Box::column().margin(1);
        assert_eq!(b.margin_top, Some(1));
        assert_eq!(b.margin_left, Some(1));
    }

    #[test]
    fn text_new_has_no_styling() {
        let t = Text::new("hi");
        assert!(!t.has_styling());
        assert_eq!(t.content, "hi");
    }

    #[test]
    fn text_styling_toggles_have_effect() {
        let t = Text::new("hi").bold().color(Color::Red);
        assert!(t.has_styling());
        assert!(t.bold);
        assert_eq!(t.color, Color::Red);
    }

    #[test]
    fn transform_default_is_zero_offset() {
        let t = Transform::new(Text::new("hi"));
        assert_eq!(t.x, 0);
        assert_eq!(t.y, 0);
    }

    #[test]
    fn transform_translate_sets_both_axes() {
        let t = Transform::new(Text::new("hi")).translate(3, 2);
        assert_eq!(t.x, 3);
        assert_eq!(t.y, 2);
    }

    #[test]
    fn static_carries_children() {
        let s = Static::new()
            .child(Text::new("line 1"))
            .child(Text::new("line 2"));
        assert_eq!(s.children.len(), 2);
    }

    #[test]
    fn box_border_style_default_uses_all_sides() {
        let b = Box::column().border_style(crate::style::BorderStyle::Round);
        assert_eq!(b.border_style, crate::style::BorderStyle::Round);
        assert!(b.borders.top);
        assert!(b.borders.bottom);
    }

    #[test]
    fn color_hex_serialises_as_string() {
        let c = Color::Hex("#ff00aa".to_string());
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains("#ff00aa"));
    }
}
