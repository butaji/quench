// main.r.tsx - Main UI view using Ratatui builder pattern.
//
// Demonstrates:
// - JSX-style widget composition
// - Layout primitives
// - Styled text rendering

import { State } from "./main.r.ts";

/// Render the main application view.
export function render(f: Frame, state: State): void {
    // Create a layout block
    let block = Block.default()
        .title("Ratatui Demo")
        .borders(Borders.ALL);
    
    // Create inner content
    let content = createContent(state);
    
    // Render the block with content
    f.render_widget(block, f.size());
    f.render_widget(content, centeredRect(f.size(), 3, 30));
}

/// Create centered content area.
function createContent(state: State): Paragraph {
    let lines: string[] = [];
    
    lines.push("Counter: " + state.counter);
    lines.push("");
    lines.push("Items:");
    
    for (let i = 0; i < state.items.length; i++) {
        const marker = i === state.selectedIndex ? ">" : " ";
        const item = state.items[i];
        lines.push(marker + " " + item);
    }
    
    lines.push("");
    lines.push("---");
    lines.push("j/k: Navigate  +/-: Counter");
    lines.push("n: New item   d: Delete");
    
    return Paragraph.new(lines.join("\n"));
}

/// Calculate a centered rectangle.
function centeredRect(area: Rect, height: number, width: number): Rect {
    let x = (area.width - width) / 2;
    let y = (area.height - height) / 2;
    return Rect.new(x, y, width, height);
}

/// Format a number for display.
function formatNum(n: number): string {
    if (n >= 0) {
        return "+" + n;
    }
    return String(n);
}
