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
//!    flow through the `taffy_bridge::style_for_box`
//!    function to Taffy.
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

// ---------------------------------------------------------------------------
// Box
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
    Center,
    FlexEnd,
    Stretch,
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

/// Ink's `<Box>` — a flexbox-style container.
///
/// Carries the full set of flexbox / position / border
/// / background props. The `taffy_bridge` module maps
/// these to `taffy::Style`; the renderer reads the
/// computed layout and draws a `Block` (or just a
/// coloured background) at the resulting rect.
///
/// `Box` does not derive `Eq` because the `f32` flex
/// growth / basis fields aren't `Eq`. Use `PartialEq` if
/// you need to compare two boxes (e.g. in a test).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Box {
    // ---- Layout: flexbox ----
    /// Layout direction.
    #[serde(default)]
    pub flex_direction: FlexDirection,
    /// Wrap mode.
    #[serde(default)]
    pub flex_wrap: FlexWrap,
    /// Flex grow factor.
    #[serde(default = "one")]
    pub flex_grow: f32,
    /// Flex shrink factor.
    #[serde(default = "one")]
    pub flex_shrink: f32,
    /// Flex basis as a percentage of the parent's main
    /// axis. `0.0` means auto-sized.
    #[serde(default)]
    pub flex_basis_pct: f32,

    // ---- Layout: size ----
    /// Width in cells. `None` = auto.
    #[serde(default)]
    pub width: Option<u16>,
    /// Height in cells. `None` = auto.
    #[serde(default)]
    pub height: Option<u16>,
    /// Min width in cells.
    #[serde(default)]
    pub min_width: Option<u16>,
    /// Min height in cells.
    #[serde(default)]
    pub min_height: Option<u16>,
    /// Max width in cells.
    #[serde(default)]
    pub max_width: Option<u16>,
    /// Max height in cells.
    #[serde(default)]
    pub max_height: Option<u16>,

    // ---- Layout: padding ----
    /// Padding on the top edge.
    #[serde(default)]
    pub padding_top: Option<u16>,
    /// Padding on the right edge.
    #[serde(default)]
    pub padding_right: Option<u16>,
    /// Padding on the bottom edge.
    #[serde(default)]
    pub padding_bottom: Option<u16>,
    /// Padding on the left edge.
    #[serde(default)]
    pub padding_left: Option<u16>,

    // ---- Layout: margin ----
    /// Margin on the top edge.
    #[serde(default)]
    pub margin_top: Option<u16>,
    /// Margin on the right edge.
    #[serde(default)]
    pub margin_right: Option<u16>,
    /// Margin on the bottom edge.
    #[serde(default)]
    pub margin_bottom: Option<u16>,
    /// Margin on the left edge.
    #[serde(default)]
    pub margin_left: Option<u16>,

    // ---- Layout: gap ----
    /// Row gap (vertical, in a `column` box).
    #[serde(default)]
    pub row_gap: Option<u16>,
    /// Column gap (horizontal, in a `row` box).
    #[serde(default)]
    pub column_gap: Option<u16>,

    // ---- Layout: alignment ----
    /// `align-items` for the children of this box.
    #[serde(default)]
    pub align_items: AlignItems,
    /// `align-self` — overrides the parent's `align-items`
    /// for this box. (Field is on `Box` for completeness
    /// even though in practice it's set on the child.)
    #[serde(default)]
    pub align_self: AlignSelf,
    /// `align-content` (only meaningful with wrap).
    #[serde(default)]
    pub align_content: AlignContent,
    /// `justify-content`.
    #[serde(default)]
    pub justify_content: JustifyContent,

    // ---- Layout: position / overflow / display ----
    /// Position mode (relative / absolute).
    #[serde(default)]
    pub position: Position,
    /// `top` inset for `position: absolute`.
    #[serde(default)]
    pub top: Option<u16>,
    /// `right` inset for `position: absolute`.
    #[serde(default)]
    pub right: Option<u16>,
    /// `bottom` inset for `position: absolute`.
    #[serde(default)]
    pub bottom: Option<u16>,
    /// `left` inset for `position: absolute`.
    #[serde(default)]
    pub left: Option<u16>,
    /// `display: none` removes the node from the layout.
    #[serde(default)]
    pub display: Display,
    /// `overflow-x`.
    #[serde(default)]
    pub overflow_x: Overflow,
    /// `overflow-y` (currently a passthrough — Ratatui
    /// doesn't have a separate Y overflow concept).
    #[serde(default)]
    pub overflow_y: Overflow,

    // ---- Decoration: border ----
    /// Which sides to draw a border on.
    #[serde(default)]
    pub borders: Borders,
    /// Border style.
    #[serde(default)]
    pub border_style: BorderStyle,
    /// Border colour (foreground).
    #[serde(default)]
    pub border_color: Option<Color>,
    /// Whether to dim the border (foreground).
    #[serde(default)]
    pub border_dim_color: bool,
    /// Border background colour.
    #[serde(default)]
    pub border_background_color: Option<Color>,

    // ---- Decoration: background ----
    /// Background fill colour.
    #[serde(default)]
    pub background_color: Option<Color>,

    // ---- Children ----
    /// Children of this box.
    #[serde(default)]
    pub children: Vec<VNode>,
}

fn one() -> f32 { 1.0 }

impl Box {
    /// Create a `Box` with the default row flex direction.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a `Box` with `flexDirection: row`.
    pub fn row() -> Self {
        Self {
            flex_direction: FlexDirection::Row,
            ..Self::default()
        }
    }

    /// Create a `Box` with `flexDirection: column`.
    pub fn column() -> Self {
        Self {
            flex_direction: FlexDirection::Column,
            ..Self::default()
        }
    }

    /// Set the flex direction.
    pub fn flex_direction(mut self, dir: FlexDirection) -> Self {
        self.flex_direction = dir;
        self
    }

    /// Set padding on all four sides at once.
    pub fn padding(mut self, p: u16) -> Self {
        self.padding_top = Some(p);
        self.padding_right = Some(p);
        self.padding_bottom = Some(p);
        self.padding_left = Some(p);
        self
    }

    /// Set margin on all four sides at once.
    pub fn margin(mut self, m: u16) -> Self {
        self.margin_top = Some(m);
        self.margin_right = Some(m);
        self.margin_bottom = Some(m);
        self.margin_left = Some(m);
        self
    }

    /// Set a fixed width.
    pub fn width(mut self, w: u16) -> Self {
        self.width = Some(w);
        self
    }

    /// Set a fixed height.
    pub fn height(mut self, h: u16) -> Self {
        self.height = Some(h);
        self
    }

    /// Set the border style and a default "all sides"
    /// configuration.
    pub fn border_style(mut self, s: BorderStyle) -> Self {
        self.border_style = s;
        if !self.borders.top && !self.borders.right && !self.borders.bottom && !self.borders.left {
            self.borders = Borders::ALL;
        }
        self
    }

    /// Set the background colour.
    pub fn background_color(mut self, c: Color) -> Self {
        self.background_color = Some(c);
        self
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

impl Default for Box {
    // allow:too_many_lines
    // The Box struct has 30+ Ink-style fields; the
    // Default impl just lists each one with its initial
    // value. Splitting it into helper builders would
    // add noise without reducing the field-by-field
    // clarity of this single source of truth.
    fn default() -> Self {
        Self {
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::NoWrap,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis_pct: 0.0,
            width: None,
            height: None,
            min_width: None,
            min_height: None,
            max_width: None,
            max_height: None,
            padding_top: None,
            padding_right: None,
            padding_bottom: None,
            padding_left: None,
            margin_top: None,
            margin_right: None,
            margin_bottom: None,
            margin_left: None,
            row_gap: None,
            column_gap: None,
            align_items: AlignItems::FlexStart,
            align_self: AlignSelf::Auto,
            align_content: AlignContent::FlexStart,
            justify_content: JustifyContent::FlexStart,
            position: Position::Relative,
            top: None,
            right: None,
            bottom: None,
            left: None,
            display: Display::Flex,
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
            borders: Borders::default(),
            border_style: BorderStyle::Single,
            border_color: None,
            border_dim_color: false,
            border_background_color: None,
            background_color: None,
            children: Vec::new(),
        }
    }
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

// ---------------------------------------------------------------------------
// Spacer
// ---------------------------------------------------------------------------

/// Ink's `<Spacer>` — a flexbox separator that fills the
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
    use super::*;

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
