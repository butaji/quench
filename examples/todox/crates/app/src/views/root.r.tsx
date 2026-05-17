// root.r.tsx - Main view component.

import { AppState, Filter, Task, filterTasks } from "../state.r.ts";

/// Render the root view.
export function render(f: Frame, state: AppState): void {
    let lines: string[] = [];
    lines.push(" TODOX - Task Manager ");
    lines.push("=".repeat(40));

    // Add header
    const filtered = filterTasks(state.tasks, state.filter);
    
    if (filtered.length === 0) {
        lines.push("(No tasks)");
    } else {
        for (let i = 0; i < filtered.length; i++) {
            const task = filtered[i];
            const marker = task.done ? "[x]" : "[ ]";
            const prefix = (state.selected === i) ? "> " : "  ";
            lines.push(prefix + marker + " " + task.title);
        }
    }

    lines.push("");
    lines.push("-".repeat(40));
    lines.push("j/k: Navigate  x: Toggle  a: Add  d: Delete");
    lines.push("f: Filter    q: Quit");

    const content = lines.join("\n");
    let para = Paragraph::new(content);
    f.render_widget(para, f.size());
}
