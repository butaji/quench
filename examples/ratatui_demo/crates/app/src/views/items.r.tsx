// src/views/items.r.tsx - Main UI view
// Demonstrates: JSX/TSX, Ratatui widgets, embedded Rust

import { AppState, Item, getTotalValue } from "../state.r.ts";

// Main render function
export function render(f: Frame, state: AppState): void {
    let lines: string[] = [];
    
    // Header
    lines.push(" ╔═══════════════════════════════════╗");
    lines.push(" ║     Inventory Management Demo     ║");
    lines.push(" ╚═══════════════════════════════════╝");
    lines.push("");
    
    // Show view-specific content
    switch (state.view.tag) {
        case "List":
            lines.push("Items:");
            lines.push("-".repeat(40));
            for (let i = 0; i < state.items.length; i++) {
                const item = state.items[i];
                const marker = state.selected === i ? "> " : "  ";
                const done = item.quantity > 0 ? "" : " [OUT OF STOCK]";
                lines.push(marker + item.name + " - $" + item.price + " x" + item.quantity + done);
            }
            break;
        case "Detail":
            lines.push("Item Detail: #" + state.view.id);
            lines.push("-".repeat(40));
            for (let i = 0; i < state.items.length; i++) {
                if (state.items[i].id === state.view.id) {
                    const item = state.items[i];
                    lines.push("Name: " + item.name);
                    lines.push("Price: $" + item.price);
                    lines.push("Quantity: " + item.quantity);
                    lines.push("Value: $" + (item.price * item.quantity));
                }
            }
            break;
        case "Edit":
            lines.push("Edit Mode");
            lines.push("-".repeat(40));
            lines.push("Press ENTER to save, ESC to cancel");
            break;
    }
    
    lines.push("");
    lines.push("-".repeat(40));
    lines.push("Total Value: $" + getTotalValue(state.items));
    lines.push("");
    lines.push("j/k: Navigate  e: Edit  d: Detail  q: Quit");
    
    // Render using Ratatui Paragraph widget
    const content = lines.join("\n");
    let para = Paragraph::new(content);
    let block = Block::default().title("Demo").borders(Borders::ALL);
    f.render_widget(para.block(block), f.size());
}
