// views/root.r.tsx - Main view component.
// Demonstrates: JSX/TSX syntax, Ratatui widget construction

import { AppState, Filter, Task, filterTasks } from "../state.r.ts";

/// Message type for view updates.
export type ViewMessage =
    | { tag: "Render"; frame: Frame }
    | { tag: "Resize" };

/// Render the root TUI view.
export function render(f: Frame, state: AppState): void {
    const lines: string[] = [];
    
    // Header
    lines.push(" ╔════════════════════════════════════╗");
    lines.push(" ║       TODOX - Task Manager         ║");
    lines.push(" ╚════════════════════════════════════╝");
    lines.push("");
    
    // Filter status
    const filterLabel = state.filter === Filter.All 
        ? "All" 
        : state.filter === Filter.Active 
            ? "Active" 
            : "Completed";
    lines.push("[Filter: " + filterLabel + "]");
    lines.push("");
    
    // Task list
    const filtered = filterTasks(state.tasks, state.filter);
    
    if (filtered.length === 0) {
        lines.push("  (No tasks)");
    } else {
        for (let i = 0; i < filtered.length; i++) {
            const task = filtered[i];
            const marker = task.done ? "[x]" : "[ ]";
            const prefix = (getOriginalIndex(state, i, filtered) === state.selected) 
                ? "> " 
                : "  ";
            lines.push(prefix + marker + " " + task.title);
        }
    }
    
    lines.push("");
    lines.push("────────────────────────────────────────");
    lines.push("  Tasks: " + state.tasks.length + " total");
    lines.push("");
    lines.push("  j/k: Navigate  x: Toggle  a: Add");
    lines.push("  d: Delete     f: Filter  q: Quit");
    
    // Render using Ratatui widgets via JSX
    const content = lines.join("\n");
    const para = Paragraph.new(content);
    const block = Block.default().title("TODOX").borders(Borders.ALL);
    f.render_widget(para.block(block), f.size());
}

/// Get original index in full task list from filtered index.
function getOriginalIndex(state: AppState, filteredIdx: number, filtered: Task[]): number {
    if (filteredIdx >= filtered.length) {
        return -1;
    }
    const task = filtered[filteredIdx];
    for (let i = 0; i < state.tasks.length; i++) {
        if (state.tasks[i].id === task.id) {
            return i;
        }
    }
    return -1;
}
