//! Ink-style component types.
//!
//! Each component is a plain struct that carries its
//! configuration (e.g. `Box`'s `flexDirection`, `Text`'s
//! `colour`). Children are stored as a `Vec<VNode>` on the
//! struct itself, so component creation is a builder
//! pattern: `Box::column().child(Text::new("hi"))`.
//!
//! The struct fields are organised in two layers:
//!
//! 1. **Container / layout** fields (only on `Box`).
//!    These are the full set of Ink flexbox props plus
//!    position / overflow / border / background. They
//!    flow through the `yoga_bridge::style_for_box`
//!    function to Yoga.
//! 2. **Leaf** fields on `Text` (colour, modifiers) and
//!    the spacer / static / transform types. The leaf
//!    types don't carry layout data; the renderer
//!    decides their size from the parent's flexbox rect.
//!
//! These types are the canonical Ink-compatible Rust API.
//! The `runts-ratatui` plugin emits Ratatui code that
//! doesn't go through these types at all — the plugin
//! produces literal `ratatui::widgets::*` calls from the
//! HIR. But the `runts-ink` types are still useful as:
//!
//!   * A **reference implementation** for what
//!     `<Box>`, `<Text>`, etc. should do.
//!   * A **runtime** for users who want to write their
//!     TUI app in pure Rust instead of `.tsx`.
//!   * A **test fixture** for plugin codegen — the
//!     plugin's output can be compared against these
//!     types' behaviour.

use serde::{Deserialize, Serialize};

use crate::style::{BorderStyle, Borders, Display, Overflow, Position};
use crate::vnode::VNode;

mod ink_box;
pub use ink_box::Box;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// Flexbox direction for a `Box`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FlexDirection {
    /// Children laid out left-to-right.
    #[default]
    Row,
    /// Children laid out top-to-bottom.
    Column,
    /// Children laid out right-to-left.
    RowReverse,
    /// Children laid out bottom-to-top.
    ColumnReverse,
}

/// Flex wrap mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FlexWrap {
    /// Children never wrap.
    #[default]
    NoWrap,
    /// Children wrap onto the next line.
    Wrap,
    /// Children wrap and the wrap order is reversed.
    WrapReverse,
}

/// `align-items` for a flexbox parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AlignItems {
    /// Pack at the start of the cross axis.
    #[default]
    FlexStart,
    /// Pack at the centre of the cross axis.
    Center,
    /// Pack at the end of the cross axis.
    FlexEnd,
    /// Stretch to fill the cross axis.
    Stretch,
    /// Align by baseline (text).
    Baseline,
}

/// `align-self` for a single child.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AlignSelf {
    /// Inherit from parent's `align-items`.
    #[default]
    Auto,
    /// See `AlignItems`.
    FlexStart,
    /// Pack at the centre of the cross axis.
    Center,
    /// Pack at the end of the cross axis.
    FlexEnd,
    /// Stretch to fill the cross axis.
    Stretch,
    /// Align by baseline (text).
    Baseline,
}

/// `align-content` for a flexbox parent with wrapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AlignContent {
    /// Pack rows at the start.
    #[default]
    FlexStart,
    /// Pack rows at the centre.
    Center,
    /// Pack rows at the end.
    FlexEnd,
    /// Stretch rows to fill.
    Stretch,
    /// Distribute rows with equal gaps.
    SpaceBetween,
    /// Distribute rows with equal surrounding gaps.
    SpaceAround,
}

/// `justify-content` for a flexbox parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum JustifyContent {
    /// Pack children at the start of the main axis.
    #[default]
    FlexStart,
    /// Pack at the centre.
    Center,
    /// Pack at the end.
    FlexEnd,
    /// Equal gaps between children.
    SpaceBetween,
    /// Equal surrounding gaps.
    SpaceAround,
    /// Equal gaps including the ends.
    SpaceEvenly,
}

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

/// Foreground colour for `Text`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Color {
    /// Default terminal foreground.
    Default,
    /// ANSI black.
    Black,
    /// ANSI red.
    Red,
    /// ANSI green.
    Green,
    /// ANSI yellow.
    Yellow,
    /// ANSI blue.
    Blue,
    /// ANSI magenta.
    Magenta,
    /// ANSI cyan.
    Cyan,
    /// ANSI white.
    White,
    /// ANSI bright black (grey).
    Gray,
    /// A 24-bit RGB colour, written as `#rrggbb`.
    #[serde(untagged)]
    Hex(String),
}

impl Default for Color {
    fn default() -> Self {
        Color::Default
    }
}

/// Ink's `<Text>` — a single line of styled text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Text {
    /// The literal text to render.
    pub content: String,
    /// Foreground colour. `Default` is the terminal default.
    #[serde(default)]
    pub color: Color,
    /// Background colour. `Default` is the terminal default.
    #[serde(default)]
    pub background_color: Color,
    /// Bold weight.
    #[serde(default)]
    pub bold: bool,
    /// Italic.
    #[serde(default)]
    pub italic: bool,
    /// Underline.
    #[serde(default)]
    pub underline: bool,
    /// Strikethrough.
    #[serde(default)]
    pub strikethrough: bool,
    /// Dimmed (faded).
    #[serde(default)]
    pub dim_color: bool,
    /// Inverted (swap fg / bg).
    #[serde(default)]
    pub inverse: bool,
    /// Wrap mode.
    #[serde(default)]
    pub wrap: crate::style::Wrap,
}

impl Text {
    /// Create a `Text` with the given content.
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            color: Color::Default,
            background_color: Color::Default,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            dim_color: false,
            inverse: false,
            wrap: crate::style::Wrap::Wrap,
        }
    }

    /// Set the foreground colour.
    pub fn color(mut self, c: Color) -> Self {
        self.color = c;
        self
    }

    /// Set the background colour.
    pub fn background_color(mut self, c: Color) -> Self {
        self.background_color = c;
        self
    }

    /// Set the bold weight.
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    /// Set italic.
    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    /// Set underline.
    pub fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    /// Set strikethrough.
    pub fn strikethrough(mut self) -> Self {
        self.strikethrough = true;
        self
    }

    /// Set dimmed.
    pub fn dim(mut self) -> Self {
        self.dim_color = true;
        self
    }

    /// Set inverted (swap fg / bg).
    pub fn inverse(mut self) -> Self {
        self.inverse = true;
        self
    }

    /// True if any styling beyond default content colour
    /// is set.
    pub fn has_styling(&self) -> bool {
        self.color != Color::Default
            || self.background_color != Color::Default
            || self.bold
            || self.italic
            || self.underline
            || self.strikethrough
            || self.dim_color
            || self.inverse
    }
}

// ---------------------------------------------------------------------------
// Newline
// ---------------------------------------------------------------------------

/// Ink's `<Newline>` — a vertical separator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Newline;

impl Newline {
    /// Constructor for symmetry with the other
    /// components. Codegen emits `Newline::new()`.
    pub fn new() -> Self {
        Self
    }
}

// ---------------------------------------------------------------------------
// Spacer
// ---------------------------------------------------------------------------

/// A flexbox separator that fills the
/// remaining main-axis space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Spacer;

impl Spacer {
    /// Create a Spacer with `flex_grow: 1.0`.
    pub fn new() -> Self {
        Self
    }
}

// ---------------------------------------------------------------------------
// Static
// ---------------------------------------------------------------------------

/// Ink's `<Static>` — a pre-rendered fragment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Static {
    /// Pre-rendered children.
    pub children: Vec<VNode>,
}

impl Static {
    /// Create an empty `Static`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a child.
    pub fn child(mut self, c: impl Into<VNode>) -> Self {
        self.children.push(c.into());
        self
    }

    /// Append many children.
    pub fn children(mut self, cs: impl IntoIterator<Item = VNode>) -> Self {
        self.children.extend(cs);
        self
    }
}

impl Default for Static {
    fn default() -> Self {
        Self { children: Vec::new() }
    }
}

// ---------------------------------------------------------------------------
// Transform
// ---------------------------------------------------------------------------

/// Ink's `<Transform>` — wrap a child in an offset / scale.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    /// X offset in cells.
    pub x: i16,
    /// Y offset in cells.
    pub y: i16,
    /// The wrapped child.
    pub child: std::boxed::Box<VNode>,
}

impl Transform {
    /// Create a `Transform` with no offset.
    pub fn new(child: impl Into<VNode>) -> Self {
        Self { x: 0, y: 0, child: std::boxed::Box::new(child.into()) }
    }

    /// Set the X offset.
    pub fn translate_x(mut self, x: i16) -> Self {
        self.x = x;
        self
    }

    /// Set the Y offset.
    pub fn translate_y(mut self, y: i16) -> Self {
        self.y = y;
        self
    }

    /// Set both offsets.
    pub fn translate(mut self, x: i16, y: i16) -> Self {
        self.x = x;
        self.y = y;
        self
    }
}

#[cfg(test)]
mod tests {
    include!("../components_tests.inc");
}
