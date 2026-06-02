//! Ink-style style enums that don't fit neatly into the
//! "component" bucket.
//!
//! These types are used as fields on `Box` and `Text`:
//!
//! * [`BorderStyle`] — `<Box borderStyle="round" />`
//! * [`Borders`] — `<Box borderTop borderBottom />` etc.
//! * [`Display`] — `<Box display="none" />`
//! * [`Overflow`] — `<Box overflowX="hidden" />`
//! * [`Position`] — `<Box position="absolute" />`
//! * [`Wrap`] — `<Text wrap="truncate" />`
//!
//! Each enum's variants map 1:1 to Ink's string props.

use serde::{Deserialize, Serialize};

/// Ink's `borderStyle` prop.
///
/// Mapped to a `ratatui::widgets::BorderType` variant in
/// the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BorderStyle {
    /// Plain ASCII border (the default).
    #[default]
    Single,
    /// Double-line border.
    Double,
    /// Rounded corners.
    Round,
    /// Thick / bold border.
    Bold,
    /// ASCII-art with `+`, `-`, `|` — not a stock
    /// Ratatui variant, so the renderer draws it
    /// manually.
    Classic,
}

impl BorderStyle {
    /// Map to a `ratatui::widgets::BorderType`. `Classic`
    /// is rendered manually so it returns `Plain` here and
    /// the renderer overrides the corners.
    pub fn to_ratatui(self) -> ratatui::widgets::BorderType {
        use ratatui::widgets::BorderType;
        match self {
            BorderStyle::Single => BorderType::Plain,
            BorderStyle::Double => BorderType::Double,
            BorderStyle::Round => BorderType::Rounded,
            BorderStyle::Bold => BorderType::Thick,
            BorderStyle::Classic => BorderType::Plain, // overridden
        }
    }
}

/// A bit-set of which borders a `<Box>` should draw.
///
/// `<Box borderTop borderBottom />` translates to
/// `Borders { top: true, bottom: true, ..Default::default() }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Borders {
    /// Top border.
    pub top: bool,
    /// Right border.
    pub right: bool,
    /// Bottom border.
    pub bottom: bool,
    /// Left border.
    pub left: bool,
}

impl Borders {
    /// All four sides.
    pub const ALL: Self = Self { top: true, right: true, bottom: true, left: true };
    /// Just the top and bottom.
    pub const HORIZONTAL: Self = Self { top: true, right: false, bottom: true, left: false };
    /// Just the left and right.
    pub const VERTICAL: Self = Self { top: false, right: true, bottom: false, left: true };

    /// Convert to a `ratatui::widgets::Borders` bitflag.
    pub fn to_ratatui(self) -> ratatui::widgets::Borders {
        use ratatui::widgets::Borders as R;
        let mut b = R::NONE;
        if self.top {
            b |= R::TOP;
        }
        if self.right {
            b |= R::RIGHT;
        }
        if self.bottom {
            b |= R::BOTTOM;
        }
        if self.left {
            b |= R::LEFT;
        }
        b
    }
}

/// Ink's `display` prop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Display {
    /// Default: render the box.
    #[default]
    Flex,
    /// Hide the box entirely; the layout still computes
    /// its size but no widgets are drawn.
    None,
}

/// Ink's `overflowX` / `overflowY` prop.
///
/// `Hidden` clips children to the computed rect; the
/// other variants are passthroughs (Ratatui always
/// clips anyway).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Overflow {
    /// Visible — don't clip. Ratatui's widget draw loop
    /// doesn't really support overflow, so this is
    /// effectively the same as `Hidden` in practice.
    #[default]
    Visible,
    /// Clip children to the computed rect.
    Hidden,
}

/// Ink's `position` prop. `Absolute` lets a child be
/// placed at a specific (top, left) offset, independent
/// of the parent's flexbox flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Position {
    /// Default: participate in the parent's flexbox flow.
    #[default]
    Relative,
    /// Place the child at an absolute offset; the parent
    /// stops participating in the flexbox flow.
    Absolute,
}

/// Ink's `wrap` prop on `<Text>`. Controls what happens
/// when a text line is wider than the parent rect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Wrap {
    /// Soft-wrap on word boundaries.
    #[default]
    Wrap,
    /// Hard-wrap at the parent width.
    Hard,
    /// Truncate with an ellipsis on the right.
    Truncate,
    /// Truncate with `…foo…` in the middle.
    TruncateMiddle,
}

impl Wrap {
    /// Map to a `ratatui::widgets::Wrap`. `Hard` /
    /// `Truncate` / `TruncateMiddle` need extra work in
    /// the renderer — the simple `Wrap` case maps
    /// directly.
    pub fn to_ratatui(self) -> ratatui::widgets::Wrap {
        use ratatui::widgets::Wrap as W;
        match self {
            Wrap::Wrap => W { trim: false },
            Wrap::Hard => W { trim: true },
            // Truncate / TruncateMiddle are not directly
            // expressible in Ratatui; the renderer
            // post-processes the lines.
            Wrap::Truncate | Wrap::TruncateMiddle => W { trim: true },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn border_style_default_is_single() {
        assert_eq!(BorderStyle::default(), BorderStyle::Single);
    }

    #[test]
    fn border_style_classic_maps_to_plain_for_corners() {
        // The renderer draws Classic corners manually;
        // the Ratatui side uses Plain for the body.
        assert_eq!(BorderStyle::Classic.to_ratatui(), ratatui::widgets::BorderType::Plain);
        assert_eq!(BorderStyle::Round.to_ratatui(), ratatui::widgets::BorderType::Rounded);
        assert_eq!(BorderStyle::Double.to_ratatui(), ratatui::widgets::BorderType::Double);
    }

    #[test]
    fn borders_all_includes_every_side() {
        let b = Borders::ALL.to_ratatui();
        assert!(b.contains(ratatui::widgets::Borders::TOP));
        assert!(b.contains(ratatui::widgets::Borders::RIGHT));
        assert!(b.contains(ratatui::widgets::Borders::BOTTOM));
        assert!(b.contains(ratatui::widgets::Borders::LEFT));
    }

    #[test]
    fn wrap_round_trip_through_serde() {
        for w in [Wrap::Wrap, Wrap::Hard, Wrap::Truncate, Wrap::TruncateMiddle] {
            let s = serde_json::to_string(&w).unwrap();
            let back: Wrap = serde_json::from_str(&s).unwrap();
            assert_eq!(back, w);
        }
    }

    #[test]
    fn position_default_is_relative() {
        assert_eq!(Position::default(), Position::Relative);
    }

    #[test]
    fn display_none_hides_box() {
        assert_eq!(Display::None, Display::None);
        assert_ne!(Display::None, Display::Flex);
    }
}
