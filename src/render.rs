//! Terminal rendering
//!
//! Renders the node tree to the ratatui terminal buffer.

pub mod color;
pub mod keycode;
pub mod text;

use crate::bridge;
use crate::ink::PropValue;
use crate::render::text::truncate_text;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, BorderType, Paragraph, Widget, Wrap},
};

pub use color::parse_color;
pub use keycode::keycode_to_ink_name;

/// Render the current tree to the terminal
pub fn render_tree(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    root_id: Option<u32>,
) -> anyhow::Result<()> {
    terminal.draw(|frame| {
        let Some(root_id) = root_id else { return };

        let area = frame.area();
        bridge::__ink_set_terminal_size(area.width as u32, area.height as u32);

        // Only recalculate layout when the tree is dirty.
        // Layout is expensive for large trees (~µs but adds up at 60fps).
        // Calling calculate_layout unconditionally burns CPU on every idle frame.
        if bridge::__ink_is_dirty() {
            if let Err(e) = bridge::__ink_calculate_layout() {
                tracing::error!("Layout error: {:?}", e);
                return;
            }
        }

        render_node(root_id, frame.buffer_mut(), area);
    })?;

    Ok(())
}

/// Recursively render a node and its children
#[allow(clippy::complexity, clippy::too_many_lines)]
fn render_node(node_id: u32, buf: &mut Buffer, area: Rect) {
    let tag = match bridge::__ink_get_node_tag(node_id) {
        Some(t) => t,
        None => return,
    };

    let layout = match bridge::__ink_get_layout(node_id) {
        Some(l) => l,
        None => return,
    };

    // Ink uses round() for positions and ceil() for dimensions
    let x = layout.0.round() as u16;
    let y = layout.1.round() as u16;
    let w = layout.2.ceil() as u16;
    let h = layout.3.ceil() as u16;

    if x >= area.right() || y >= area.bottom() {
        return;
    }

    match tag.as_str() {
        "ink-box" => render_box(node_id, buf, x, y, w, h),
        "ink-text" => render_text(node_id, buf, x, y, w, h),
        "ink-static" => render_static(node_id, buf, area),
        "ink-newline" => render_newline(buf, x, y, w, h),
        "ink-spacer" => {} // Spacer is invisible
        _ => {}
    }

    // Render children (except static which renders them inline)
    if tag.as_str() != "ink-static" {
        if let Some(children) = bridge::__ink_get_node_children(node_id) {
            for &child_id in &children {
                render_node(child_id, buf, area);
            }
        }
    }
}

/// Render a box node (ink-box)
fn render_box(node_id: u32, buf: &mut Buffer, x: u16, y: u16, w: u16, h: u16) {
    let mut block = Block::default();

    // Handle border styles
    block = apply_border_styles(node_id, block);

    // Handle padding
    block = apply_padding(node_id, block);

    // Handle title
    if let Some(title) = bridge::__ink_get_node_prop(node_id, "title")
        .map(|s| s.trim_matches('"').to_string())
    {
        block = block.title(title);
    }

    // Handle background color
    let bg_color = bridge::__ink_get_node_prop(node_id, "backgroundColor")
        .map(|s| s.trim_matches('"').to_string())
        .and_then(|s| parse_color(&s));

    let rect = Rect::new(x, y, w, h);

    if let Some(bg) = bg_color {
        block = block.style(Style::default().bg(bg));
        fill_background(buf, rect, bg);
    }

    block.render(rect, buf);
}

/// Apply border styles to a block
#[allow(clippy::complexity, clippy::too_many_lines)]
fn apply_border_styles(node_id: u32, mut block: Block) -> Block {
    let border_style = bridge::__ink_get_node_prop(node_id, "borderStyle")
        .map(|s| s.trim_matches('"').to_string());

    let border_top = matches!(
        bridge::__ink_get_node_prop_raw(node_id, "borderTop"),
        Some(PropValue::Bool(true))
    );
    let border_bottom = matches!(
        bridge::__ink_get_node_prop_raw(node_id, "borderBottom"),
        Some(PropValue::Bool(true))
    );
    let border_left = matches!(
        bridge::__ink_get_node_prop_raw(node_id, "borderLeft"),
        Some(PropValue::Bool(true))
    );
    let border_right = matches!(
        bridge::__ink_get_node_prop_raw(node_id, "borderRight"),
        Some(PropValue::Bool(true))
    );

    let border_color = bridge::__ink_get_node_prop(node_id, "borderColor")
        .map(|s| s.trim_matches('"').to_string())
        .and_then(|s| parse_color(&s));
    let _border_dim_color = bridge::__ink_get_node_prop(node_id, "borderDimColor")
        .map(|s| s.trim_matches('"').to_string())
        .and_then(|s| parse_color(&s));

    let has_individual_borders = border_top || border_bottom || border_left || border_right;

    if has_individual_borders || border_style.is_some() {
        let border_type = border_style
            .as_ref()
            .map(|s| match s.as_str() {
                "round" => BorderType::Rounded,
                "bold" => BorderType::Thick,
                "double" => BorderType::Double,
                _ => BorderType::Plain,
            })
            .unwrap_or(BorderType::Plain);

        let borders = if has_individual_borders {
            let mut b = Borders::empty();
            if border_top {
                b.insert(Borders::TOP);
            }
            if border_bottom {
                b.insert(Borders::BOTTOM);
            }
            if border_left {
                b.insert(Borders::LEFT);
            }
            if border_right {
                b.insert(Borders::RIGHT);
            }
            b
        } else {
            Borders::ALL
        };

        block = block.borders(borders).border_type(border_type);

        // Apply border color
        let mut border_sty = Style::default();
        if let Some(color) = border_color {
            border_sty = border_sty.fg(color);
        }
        // Note: borderDimColor is partially supported due to ratatui limitations
        if border_sty != Style::default() {
            block = block.border_style(border_sty);
        }
    }

    block
}

/// Apply padding to a block
fn apply_padding(node_id: u32, block: Block) -> Block {
    if let Some(PropValue::Number(padding)) = bridge::__ink_get_node_prop_raw(node_id, "padding") {
        let p = padding as u16;
        if p > 0 {
            return block.padding(ratatui::widgets::Padding::symmetric(p, p));
        }
    }

    if let (Some(PropValue::Number(py)), Some(PropValue::Number(px))) = (
        bridge::__ink_get_node_prop_raw(node_id, "paddingY"),
        bridge::__ink_get_node_prop_raw(node_id, "paddingX"),
    ) {
        return block.padding(ratatui::widgets::Padding::symmetric(py as u16, px as u16));
    }

    block
}

/// Fill background for inner area (Block doesn't fill inner)
fn fill_background(buf: &mut Buffer, rect: Rect, color: Color) {
    for cy in rect.y..rect.bottom() {
        for cx in rect.x..rect.right() {
            if cx < buf.area.right() && cy < buf.area.bottom() {
                if let Some(cell) = buf.cell_mut((cx, cy)) {
                    cell.set_bg(color);
                }
            }
        }
    }
}

/// Render a text node (ink-text)
fn render_text(node_id: u32, buf: &mut Buffer, x: u16, y: u16, w: u16, h: u16) {
    let text = bridge::__ink_get_node_text(node_id).unwrap_or_default();
    let mut style = Style::default();

    style = apply_text_style(node_id, style);
    let text = apply_text_transform(node_id, text);
    let wrap = apply_text_wrap(node_id);

    // Handle truncation modes - pre-truncate text since ratatui doesn't support Wrap::Truncate
    let text = if wrap.is_none() {
        let max_chars = w.saturating_sub(1) as usize; // Leave room for potential ellipsis
        truncate_text(&text, max_chars)
    } else {
        text
    };

    let mut para = Paragraph::new(text.as_str()).style(style);
    if let Some(wrap_mode) = wrap {
        para = para.wrap(wrap_mode);
    }
    let rect = Rect::new(x, y, w, h);
    para.render(rect, buf);
}

/// Apply text wrap mode (supports both Ink 6 "textWrap" and Ink 7 "wrap")
fn apply_text_wrap(node_id: u32) -> Option<Wrap> {
    // Check both textWrap (Ink 6) and wrap (Ink 7) props
    let wrap_prop = bridge::__ink_get_node_prop(node_id, "wrap")
        .or_else(|| bridge::__ink_get_node_prop(node_id, "textWrap"))?
        .trim_matches('"')
        .to_string();

    match wrap_prop.as_str() {
        // Ink 6/Ink 7 basic modes - use word wrap
        "wrap" | "hard" | "end" | "middle" => Some(Wrap { trim: false }),
        // Truncation modes (Ink 6 and 7) - no wrap, handled in render_text
        "truncate" | "ellipsis" | "truncate-end" | "truncate-middle" | "truncate-start" => None,
        // Scroll is not supported in ratatui, fall back to wrap
        "scroll" => Some(Wrap { trim: false }),
        // Unknown modes default to wrap
        _ => Some(Wrap { trim: false }),
    }
}

/// Apply text styling props
fn apply_text_style(node_id: u32, mut style: Style) -> Style {
    if let Some(color) = bridge::__ink_get_node_prop(node_id, "color")
        .map(|s| s.trim_matches('"').to_string())
        .and_then(|s| parse_color(&s))
    {
        style = style.fg(color);
    }

    if let Some(bg_color) = bridge::__ink_get_node_prop(node_id, "backgroundColor")
        .map(|s| s.trim_matches('"').to_string())
        .and_then(|s| parse_color(&s))
    {
        style = style.bg(bg_color);
    }

    // Check prop value, not just presence.  Using .is_some() would enable
    // modifiers for ANY prop value including "false" and 0.
    if matches!(
        bridge::__ink_get_node_prop_raw(node_id, "bold"),
        Some(PropValue::Bool(true))
    ) {
        style = style.add_modifier(Modifier::BOLD);
    }
    if matches!(
        bridge::__ink_get_node_prop_raw(node_id, "dimColor"),
        Some(PropValue::Bool(true))
    ) {
        style = style.add_modifier(Modifier::DIM);
    }
    if matches!(
        bridge::__ink_get_node_prop_raw(node_id, "italic"),
        Some(PropValue::Bool(true))
    ) {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if matches!(
        bridge::__ink_get_node_prop_raw(node_id, "strikethrough"),
        Some(PropValue::Bool(true))
    ) {
        style = style.add_modifier(Modifier::CROSSED_OUT);
    }
    if matches!(
        bridge::__ink_get_node_prop_raw(node_id, "underline"),
        Some(PropValue::Bool(true))
    ) {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if matches!(
        bridge::__ink_get_node_prop_raw(node_id, "inverse"),
        Some(PropValue::Bool(true))
    ) {
        style = style.add_modifier(Modifier::REVERSED);
    }
    // small text - rendered as dim (terminals don't have small font)
    if matches!(
        bridge::__ink_get_node_prop_raw(node_id, "small"),
        Some(PropValue::Bool(true))
    ) {
        style = style.add_modifier(Modifier::DIM);
    }

    style
}

/// Apply text transform (uppercase/lowercase)
fn apply_text_transform(node_id: u32, text: String) -> String {
    match bridge::__ink_get_node_prop(node_id, "transform")
        .map(|s| s.trim_matches('"').to_string())
        .as_deref()
    {
        Some("uppercase") => text.to_uppercase(),
        Some("lowercase") => text.to_lowercase(),
        _ => text,
    }
}

/// Render static node (renders children directly)
fn render_static(node_id: u32, buf: &mut Buffer, area: Rect) {
    for &child_id in &bridge::__ink_get_node_children(node_id).unwrap_or_default() {
        render_node(child_id, buf, area);
    }
}

/// Render newline (empty paragraph)
fn render_newline(buf: &mut Buffer, x: u16, y: u16, w: u16, h: u16) {
    let rect = Rect::new(x, y, w.max(1), h.max(1));
    Paragraph::new("").render(rect, buf);
}
