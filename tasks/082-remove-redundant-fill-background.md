# Task 082: Remove Redundant fill_background

## Status: 🟠 **POLISH — NOT STARTED**

## Goal
Remove the manual `fill_background()` function in `render.rs` and let ratatui's `Block` handle background filling.

## Problem

`render.rs` manually iterates every cell in a box's rectangle to set the background color:

```rust
fn render_box(node_id: u32, buf: &mut Buffer, x: u16, y: u16, w: u16, h: u16) {
    // ...
    if let Some(bg) = bg_color {
        block = block.style(Style::default().bg(bg));
        fill_background(buf, rect, bg);  // MANUAL CELL ITERATION
    }
    block.render(rect, buf);
}

fn fill_background(buf: &mut Buffer, rect: Rect, color: Color) {
    for cy in rect.y..rect.bottom() {
        for cx in rect.x..rect.right() {
            if let Some(cell) = buf.cell_mut((cx, cy)) {
                cell.set_bg(color);
            }
        }
    }
}
```

`Block::render()` already fills its entire area with the style's background color. The manual loop:
- Is redundant — ratatui handles this
- Is slower — O(width × height) cell mutations per box
- Can conflict with `Block`'s inner area rendering

## Fix

Simply remove `fill_background()` and rely on `Block::style().bg()`:

```rust
fn render_box(node_id: u32, buf: &mut Buffer, x: u16, y: u16, w: u16, h: u16) {
    let mut block = Block::default();
    block = apply_border_styles(node_id, block);
    block = apply_padding(node_id, block);
    // ...

    let mut style = Style::default();
    if let Some(bg) = bg_color {
        style = style.bg(bg);
    }
    block = block.style(style);

    block.render(rect, buf);  // Block fills its own area
}
```

If the intent is to fill only the *inner* area (excluding borders), use `block.inner(rect)` after rendering, but this is usually unnecessary since child nodes will overwrite the inner area anyway.

## Acceptance Criteria
- [ ] `fill_background()` function is deleted from `render.rs`
- [ ] `render_box()` sets background via `Block::style(Style::default().bg(color))`
- [ ] Boxes with `backgroundColor` still render correctly
- [ ] Nested boxes with different background colors render correctly
- [ ] All rendering tests pass

## Files to Modify
- `src/render.rs` — Remove `fill_background`, update `render_box`

## References
- ratatui Block::style: https://docs.rs/ratatui/latest/ratatui/widgets/struct.Block.html
