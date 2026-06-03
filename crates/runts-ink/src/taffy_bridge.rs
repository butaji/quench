//! Bridge from runts-ink's `Box` / `Text` types to Taffy
//! flexbox styles and Ratatui render instructions.
//!
//! allow:too_many_lines
//! allow:complexity
//!
//! In Ink, Yoga (the JS Yoga) computes layout from the
//! user's `<Box>` props. We use Taffy, a Rust-native
//! flexbox/grid engine with the same semantics, instead
//! of Yoga (which has no Rust binding).
//!
//! `style_for_box` is the canonical mapping from
//! `BoxProps` (a flat field set mirroring Ink's JSX
//! props) to a `taffy::Style`. `apply_style` then takes
//! the same `BoxProps` and applies them to a Taffy
//! node, including `display: none` short-circuits.

use taffy::prelude::*;

use crate::components::Box as InkBox;
use crate::style::{BorderStyle, Borders, Display, Overflow, Position};

/// Build a Taffy `Style` from a `Box` instance.
pub fn style_for_box(b: &InkBox) -> taffy::Style {
    let mut s = taffy::Style::default();

    s.display = match b.display {
        Display::Flex => taffy::Display::Flex,
        Display::None => taffy::Display::None,
    };

    s.position = match b.position {
        Position::Relative => taffy::Position::Relative,
        Position::Absolute => taffy::Position::Absolute,
    };

    s.flex_direction = match b.flex_direction {
        crate::components::FlexDirection::Row => taffy::FlexDirection::Row,
        crate::components::FlexDirection::Column => taffy::FlexDirection::Column,
        crate::components::FlexDirection::RowReverse => taffy::FlexDirection::RowReverse,
        crate::components::FlexDirection::ColumnReverse => taffy::FlexDirection::ColumnReverse,
    };

    s.flex_wrap = match b.flex_wrap {
        crate::components::FlexWrap::NoWrap => taffy::FlexWrap::NoWrap,
        crate::components::FlexWrap::Wrap => taffy::FlexWrap::Wrap,
        crate::components::FlexWrap::WrapReverse => taffy::FlexWrap::WrapReverse,
    };

    // Default to `flex_grow: 1.0` so a Box fills
    // the available space along the parent's main
    // axis. Without this, a Box with no explicit
    // size in a column flex would collapse to the
    // height of its content (which can be 0 for
    // `auto`-sized children — the same circular
    // dependency the size default avoids). Users
    // who want fixed-size boxes can set
    // `flex_grow: 0` explicitly.
    s.flex_grow = b.flex_grow;
    s.flex_shrink = b.flex_shrink;
    s.flex_basis = taffy::Dimension::percent(b.flex_basis_pct);

    // Ink semantics: Boxes are auto-sized by
    // default (shrink-to-fit content). The
    // viewport is provided as a MaxContent-sized
    // parent so each Box collapses to its
    // content's intrinsic size. Setting
    // `Dimension::AUTO` here lets each Box size
    // to its content; since the root is the only
    // direct child of the viewport and it is
    // auto-sized, the root will be the size of
    // its largest content line.
    s.size = taffy::Size {
        width: match b.width {
            Some(w) => taffy::Dimension::length(w as f32),
            None => taffy::Dimension::AUTO,
        },
        height: match b.height {
            Some(h) => taffy::Dimension::length(h as f32),
            None => taffy::Dimension::AUTO,
        },
    };
    s.min_size = taffy::Size {
        width: dim_from(b.min_width),
        height: dim_from(b.min_height),
    };
    s.max_size = taffy::Size {
        width: dim_from(b.max_width),
        height: dim_from(b.max_height),
    };

    s.padding = taffy::Rect {
        left: length_from(b.padding_left),
        right: length_from(b.padding_right),
        top: length_from(b.padding_top),
        bottom: length_from(b.padding_bottom),
    };
    s.margin = taffy::Rect {
        left: length_auto_from(b.margin_left),
        right: length_auto_from(b.margin_right),
        top: length_auto_from(b.margin_top),
        bottom: length_auto_from(b.margin_bottom),
    };
    s.gap = taffy::Size {
        width: length_from(b.column_gap),
        height: length_from(b.row_gap),
    };

    s.align_items = Some(align_items_from(b.align_items));
    s.align_self = Some(align_self_from(b.align_self));
    s.align_content = Some(align_content_from(b.align_content));
    s.justify_content = Some(justify_content_from(b.justify_content));

    s.inset = taffy::Rect {
        left: length_auto_from(b.left),
        right: length_auto_from(b.right),
        top: length_auto_from(b.top),
        bottom: length_auto_from(b.bottom),
    };

    s.overflow = taffy::Point {
        x: overflow_to_taffy(b.overflow_x),
        y: overflow_to_taffy(b.overflow_y),
    };

    s
}

/// Build a Taffy `Style` for a `<Text>` leaf node.
///
/// A `<Text>` is sized to its intrinsic content
/// (the string length × 1 row). We use
/// `Dimension::AUTO` on both axes so Taffy can
/// measure the text's intrinsic size and
/// propagate that to the parent Box's auto-
/// sizing.
///
/// Setting `percent(1.0)` here would make a
/// `<Text>` stretch to fill the parent, which
/// contradicts Ink's shrink-to-fit semantics.
pub fn style_for_text() -> taffy::Style {
    let mut s = taffy::Style::default();
    s.display = taffy::Display::Block;
    s.size = taffy::Size {
        width: taffy::Dimension::AUTO,
        height: taffy::Dimension::AUTO,
    };
    s
}

/// Build a Taffy `Style` for a `<Spacer>`.
pub fn style_for_spacer(flex_grow: f32) -> taffy::Style {
    let mut s = taffy::Style::default();
    s.display = taffy::Display::Flex;
    s.flex_grow = flex_grow;
    s
}

fn overflow_to_taffy(o: Overflow) -> taffy::Overflow {
    match o {
        Overflow::Visible => taffy::Overflow::Visible,
        Overflow::Hidden => taffy::Overflow::Hidden,
    }
}

pub fn dim_from(opt: Option<u16>) -> taffy::Dimension {
    match opt {
        Some(px) => taffy::Dimension::length(px as f32),
        None => taffy::Dimension::AUTO,
    }
}

pub fn length_from(opt: Option<u16>) -> taffy::LengthPercentage {
    match opt {
        Some(px) => taffy::LengthPercentage::length(px as f32),
        None => taffy::LengthPercentage::length(0.0),
    }
}

pub fn length_auto_from(opt: Option<u16>) -> taffy::LengthPercentageAuto {
    match opt {
        Some(px) => taffy::LengthPercentageAuto::length(px as f32),
        None => taffy::LengthPercentageAuto::AUTO,
    }
}

fn align_items_from(a: crate::components::AlignItems) -> taffy::AlignItems {
    use crate::components::AlignItems as A;
    use taffy::AlignItems as T;
    match a {
        A::FlexStart => T::FlexStart,
        A::Center => T::Center,
        A::FlexEnd => T::FlexEnd,
        A::Stretch => T::Stretch,
        A::Baseline => T::Baseline,
    }
}

fn align_self_from(a: crate::components::AlignSelf) -> taffy::AlignSelf {
    use crate::components::AlignSelf as A;
    use taffy::AlignItems as T;
    match a {
        A::Auto => T::Start,
        A::FlexStart => T::FlexStart,
        A::Center => T::Center,
        A::FlexEnd => T::FlexEnd,
        A::Stretch => T::Stretch,
        A::Baseline => T::Baseline,
    }
}

fn align_content_from(a: crate::components::AlignContent) -> taffy::AlignContent {
    use crate::components::AlignContent as A;
    use taffy::AlignContent as T;
    match a {
        A::FlexStart => T::FlexStart,
        A::Center => T::Center,
        A::FlexEnd => T::FlexEnd,
        A::Stretch => T::Stretch,
        A::SpaceBetween => T::SpaceBetween,
        A::SpaceAround => T::SpaceAround,
    }
}

fn justify_content_from(j: crate::components::JustifyContent) -> taffy::JustifyContent {
    use crate::components::JustifyContent as J;
    use taffy::JustifyContent as T;
    match j {
        J::FlexStart => T::FlexStart,
        J::Center => T::Center,
        J::FlexEnd => T::FlexEnd,
        J::SpaceBetween => T::SpaceBetween,
        J::SpaceAround => T::SpaceAround,
        J::SpaceEvenly => T::SpaceEvenly,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{Box as InkBox, FlexDirection};
    use crate::style::{Borders, Display, Position};

    #[test]
    fn default_box_style_is_row() {
        let b = InkBox::default();
        let s = style_for_box(&b);
        assert_eq!(s.flex_direction, taffy::FlexDirection::Row);
        assert_eq!(s.display, taffy::Display::Flex);
        assert_eq!(s.position, taffy::Position::Relative);
    }

    #[test]
    fn display_none_translates_to_taffy_none() {
        let mut b = InkBox::default();
        b.display = Display::None;
        let s = style_for_box(&b);
        assert_eq!(s.display, taffy::Display::None);
    }

    #[test]
    fn position_absolute_translates() {
        let mut b = InkBox::default();
        b.position = Position::Absolute;
        b.left = Some(5);
        b.top = Some(2);
        let s = style_for_box(&b);
        assert_eq!(s.position, taffy::Position::Absolute);
    }

    #[test]
    fn flex_direction_columns() {
        let mut b = InkBox::default();
        b.flex_direction = FlexDirection::Column;
        let s = style_for_box(&b);
        assert_eq!(s.flex_direction, taffy::FlexDirection::Column);
    }

    #[test]
    fn borders_to_ratatui() {
        let mut b = Borders::default();
        b.top = true;
        b.bottom = true;
        let r = b.to_ratatui();
        assert!(r.contains(ratatui::widgets::Borders::TOP));
        assert!(r.contains(ratatui::widgets::Borders::BOTTOM));
        assert!(!r.contains(ratatui::widgets::Borders::LEFT));
        assert!(!r.contains(ratatui::widgets::Borders::RIGHT));
    }

    #[test]
    fn spacer_style_has_flex_grow() {
        let s = style_for_spacer(1.0);
        assert_eq!(s.flex_grow, 1.0);
        assert_eq!(s.display, taffy::Display::Flex);
    }

    #[test]
    fn text_style_is_block() {
        let s = style_for_text();
        assert_eq!(s.display, taffy::Display::Block);
    }
}

#[allow(dead_code)]
fn _border_style_marker(_: BorderStyle) {}
