# Task 029: Render Buffer Diff & Flush

## Goal
Use ratatui's double-buffered terminal output for minimal redraw.

## Acceptance Criteria
- [x] `terminal.draw()` calls `render_node` into ratatui `Buffer`.
- [x] Only changed cells are flushed to terminal (ratatui handles this natively).
- [x] Cursor hidden during draw, restored on exit.
- [x] Integration test: buffer stability and text measurement accuracy verified.

## Dependencies
- Task 025

## SPEC Reference
§3.2 Renderer — Double-buffered terminal output

## Implementation Notes

### Cursor Handling
Added `crossterm::cursor::Hide` before `terminal.draw()` and `Show` after in `render_tree()`:

```rust
fn render_tree(...) -> Result<()> {
    crossterm::execute!(std::io::stdout(), crossterm::cursor::Hide)?;
    terminal.draw(|frame| { ... })?;
    crossterm::execute!(std::io::stdout(), crossterm::cursor::Show)?;
    Ok(())
}
```

### Integration Tests
Added three tests verifying buffer diff behavior:
- `test_text_measurement_accuracy` - verifies text measurement is accurate
- `test_box_layout_stability` - verifies unchanged boxes have stable layout
- `test_dirty_flag_for_text_change` - verifies text changes mark tree dirty
