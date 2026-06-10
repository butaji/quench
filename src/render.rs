//! Terminal rendering
//!
//! Renders the node tree to the ratatui terminal buffer.

use crate::bridge;
use crate::ink::PropValue;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, BorderType, Paragraph, Widget},
};

/// Render the current tree to the terminal
pub fn render_tree(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    root_id: Option<u32>,
) -> anyhow::Result<()> {
    terminal.draw(|frame| {
        let Some(root_id) = root_id else { return };

        let area = frame.area();
        bridge::__ink_set_terminal_size(area.width as u32, area.height as u32);

        if let Err(e) = bridge::__ink_calculate_layout() {
            tracing::error!("Layout error: {:?}", e);
            return;
        }

        render_node(root_id, frame.buffer_mut(), area);
    })?;

    Ok(())
}

/// Recursively render a node and its children
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
fn apply_padding(node_id: u32, mut block: Block) -> Block {
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

    let para = Paragraph::new(text.as_str()).style(style);
    let rect = Rect::new(x, y, w, h);
    para.render(rect, buf);
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

    if bridge::__ink_get_node_prop(node_id, "bold").is_some() {
        style = style.add_modifier(Modifier::BOLD);
    }
    if bridge::__ink_get_node_prop(node_id, "dimColor").is_some() {
        style = style.add_modifier(Modifier::DIM);
    }
    if bridge::__ink_get_node_prop(node_id, "italic").is_some() {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if bridge::__ink_get_node_prop(node_id, "strikethrough").is_some() {
        style = style.add_modifier(Modifier::CROSSED_OUT);
    }
    if bridge::__ink_get_node_prop(node_id, "underline").is_some() {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if bridge::__ink_get_node_prop(node_id, "inverse").is_some() {
        style = style.add_modifier(Modifier::REVERSED);
    }
    // small text - rendered as dim (terminals don't have small font)
    if bridge::__ink_get_node_prop(node_id, "small").is_some() {
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

// ===================================================================
// Color parsing
// ===================================================================

/// Parse color string to ratatui Color
pub fn parse_color(s: &str) -> Option<Color> {
    match s.to_lowercase().as_str() {
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "white" => Some(Color::White),
        "gray" | "grey" => Some(Color::Gray),
        "brightblack" | "brightBlack" => Some(Color::Indexed(8)),
        "brightred" | "brightRed" => Some(Color::Indexed(9)),
        "brightgreen" | "brightGreen" => Some(Color::Indexed(10)),
        "brightyellow" | "brightYellow" => Some(Color::Indexed(11)),
        "brightblue" | "brightBlue" => Some(Color::Indexed(12)),
        "brightmagenta" | "brightMagenta" => Some(Color::Indexed(13)),
        "brightcyan" | "brightCyan" => Some(Color::Indexed(14)),
        "brightwhite" | "brightWhite" => Some(Color::Indexed(15)),
        _ => parse_hex_color(s),
    }
}

/// Parse hex color (#rgb or #rrggbb) to ratatui Color
fn parse_hex_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if !s.starts_with('#') {
        return None;
    }
    let hex = &s[1..];
    match hex.len() {
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color::Rgb(r, g, b))
        }
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            Some(Color::Rgb(r, g, b))
        }
        _ => None,
    }
}

// ===================================================================
// Key code conversion
// ===================================================================

/// Convert crossterm KeyCode to Ink-compatible key name
pub fn keycode_to_ink_name(key: &crossterm::event::KeyEvent) -> String {
    use crossterm::event::KeyCode;
    match key.code {
        KeyCode::Char(' ') => " ".to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "return".to_string(),
        KeyCode::Esc => "escape".to_string(),
        KeyCode::Backspace => "backspace".to_string(),
        KeyCode::Delete => "delete".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::Up => "upArrow".to_string(),
        KeyCode::Down => "downArrow".to_string(),
        KeyCode::Left => "leftArrow".to_string(),
        KeyCode::Right => "rightArrow".to_string(),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        KeyCode::PageUp => "pageUp".to_string(),
        KeyCode::PageDown => "pageDown".to_string(),
        KeyCode::Insert => "insert".to_string(),
        KeyCode::BackTab => "tab".to_string(),
        KeyCode::F(n) => format!("f{}", n),
        _ => format!("{:?}", key.code).to_lowercase(),
    }
}
