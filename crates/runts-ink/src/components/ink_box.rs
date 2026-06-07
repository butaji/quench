use super::{AlignContent, AlignItems, AlignSelf, BorderStyle, Borders, Color, Display, FlexDirection, FlexWrap, JustifyContent, Overflow, Position};
use crate::vnode::VNode;
use serde::{Deserialize, Serialize};

/// Ink's `<Box>` — a flexbox container.
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
    /// Z-index for stacking order.
    #[serde(default)]
    pub z_index: i16,

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

    /// Set the flex wrap mode.
    pub fn flex_wrap(mut self, wrap: FlexWrap) -> Self {
        self.flex_wrap = wrap;
        self
    }

    /// Set the position mode (relative / absolute).
    pub fn position(mut self, pos: Position) -> Self {
        self.position = pos;
        self
    }

    /// Set the top inset (for absolute positioning).
    pub fn top(mut self, v: u16) -> Self {
        self.top = Some(v);
        self
    }

    /// Set the left inset (for absolute positioning).
    pub fn left(mut self, v: u16) -> Self {
        self.left = Some(v);
        self
    }

    /// Set the right inset (for absolute positioning).
    pub fn right(mut self, v: u16) -> Self {
        self.right = Some(v);
        self
    }

    /// Set the bottom inset (for absolute positioning).
    pub fn bottom(mut self, v: u16) -> Self {
        self.bottom = Some(v);
        self
    }

    /// Set the flex-grow factor.
    pub fn flex_grow(mut self, grow: f32) -> Self {
        self.flex_grow = grow;
        self
    }

    /// Set the justify-content (main-axis
    /// alignment) of the children.
    pub fn justify_content(
        mut self,
        j: JustifyContent,
    ) -> Self {
        self.justify_content = j;
        self
    }

    /// Set the align-items (cross-axis
    /// alignment) of the children.
    pub fn align_items(mut self, a: AlignItems) -> Self {
        self.align_items = a;
        self
    }

    /// Set align-self for this box.
    pub fn align_self(mut self, a: AlignSelf) -> Self {
        self.align_self = a;
        self
    }

    /// Set the display mode (flex / none).
    pub fn display(mut self, d: Display) -> Self {
        self.display = d;
        self
    }

    /// Set the overflow-x mode.
    pub fn overflow_x(mut self, o: Overflow) -> Self {
        self.overflow_x = o;
        self
    }

    /// Set the column gap (horizontal spacing).
    pub fn column_gap(mut self, g: u16) -> Self {
        self.column_gap = Some(g);
        self
    }

    /// Set the row gap (vertical spacing).
    pub fn row_gap(mut self, g: u16) -> Self {
        self.row_gap = Some(g);
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

    /// Set padding on the X axis (left and right)
    /// at once. Mirrors the Ink `paddingX` shorthand.
    pub fn padding_x(mut self, p: u16) -> Self {
        self.padding_left = Some(p);
        self.padding_right = Some(p);
        self
    }

    /// Set padding on the Y axis (top and bottom)
    /// at once. Mirrors the Ink `paddingY` shorthand.
    pub fn padding_y(mut self, p: u16) -> Self {
        self.padding_top = Some(p);
        self.padding_bottom = Some(p);
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
    /// Set margin on the X axis (left and right).
    pub fn margin_x(mut self, m: u16) -> Self {
        self.margin_left = Some(m);
        self.margin_right = Some(m);
        self
    }
    /// Set margin on the Y axis (top and bottom).
    pub fn margin_y(mut self, m: u16) -> Self {
        self.margin_top = Some(m);
        self.margin_bottom = Some(m);
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

    /// Set the minimum width.
    pub fn min_width(mut self, w: u16) -> Self {
        self.min_width = Some(w);
        self
    }

    /// Set the minimum height.
    pub fn min_height(mut self, h: u16) -> Self {
        self.min_height = Some(h);
        self
    }

    /// Set the maximum width.
    pub fn max_width(mut self, w: u16) -> Self {
        self.max_width = Some(w);
        self
    }

    /// Set the maximum height.
    pub fn max_height(mut self, h: u16) -> Self {
        self.max_height = Some(h);
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
    /// Set the z-index.
    pub fn z_index(mut self, z: i16) -> Self {
        self.z_index = z;
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
    // The Box struct has 30+ Ink-style fields; the
    // Default impl just lists each one with its initial
    // value. Splitting it into helper builders would
    // add noise without reducing the field-by-field
    // clarity of this single source of truth.
    fn default() -> Self { Self { flex_direction: FlexDirection::Row, flex_wrap: FlexWrap::NoWrap, flex_grow: 0.0, flex_shrink: 1.0, flex_basis_pct: 0.0, width: None, height: None, min_width: None, min_height: None, max_width: None, max_height: None, padding_top: None, padding_right: None, padding_bottom: None, padding_left: None, margin_top: None, margin_right: None, margin_bottom: None, margin_left: None, row_gap: None, column_gap: None, align_items: AlignItems::Stretch, align_self: AlignSelf::Auto, align_content: AlignContent::FlexStart, justify_content: JustifyContent::FlexStart, position: Position::Relative, top: None, right: None, bottom: None, left: None, display: Display::Flex, overflow_x: Overflow::Visible, overflow_y: Overflow::Visible, borders: Borders::default(), border_style: BorderStyle::Single, border_color: None, border_dim_color: false, border_background_color: None, background_color: None, z_index: 0, children: Vec::new() } }
}
