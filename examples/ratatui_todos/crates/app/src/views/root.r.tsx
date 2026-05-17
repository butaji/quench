// views/root.r.tsx - TodoX UI Components
//
// Demonstrates JSX/TSX transpilation to Ratatui widgets:
// - JSX expressions map to function calls
// - JSX attributes map to function parameters
// - Widget composition via nested JSX

import { AppState, Filter, Task, filterTasks, countTasks } from "../main.r.ts";

/// Frame type for Ratatui rendering.
export type Frame = {
    render_widget: (widget: Widget, area: Rect) => void;
    size: () => Rect;
};

/// Rectangle type for layout.
export type Rect = {
    width: number;
    height: number;
    x: number;
    y: number;
};

/// Widget types supported by JSX transpiler.
export type Widget = 
    | { type: "Paragraph"; props: ParagraphProps }
    | { type: "Block"; props: BlockProps }
    | { type: "VBox"; children: Widget[] }
    | { type: "HBox"; children: Widget[] };

export type ParagraphProps = {
    text: string;
};

export type BlockProps = {
    title?: string;
    borders?: string;
};

/// Paragraph widget.
export function Paragraph(props: ParagraphProps): Widget {
    return { type: "Paragraph", props };
}

/// Block widget wrapper.
export function Block(props: BlockProps): Widget {
    return { type: "Block", props };
}

/// Vertical box container.
export function VBox(children: Widget[]): Widget {
    return { type: "VBox", children };
}

/// Horizontal box container.
export function HBox(children: Widget[]): Widget {
    return { type: "HBox", children };
}

/// Render the main application view.
export function renderView(f: Frame, state: AppState): void {
    const filteredTasks = filterTasks(state.tasks, state.filter);
    const counts = countTasks(state.tasks);

    // Build header
    const header = buildHeader(state);
    const headerPara = <Paragraph text={header} />;

    // Build task list
    const taskList = buildTaskList(state, filteredTasks);
    const taskPara = <Paragraph text={taskList} />;

    // Build footer with stats
    const footer = buildFooter(counts, state.filter);
    const footerPara = <Paragraph text={footer} />;

    // Compose the layout
    const content = VBox([
        headerPara,
        taskPara,
        footerPara,
    ]);

    // Wrap in block and render
    const block = <Block title="TodoX" borders="ALL" />;
    f.render_widget(content, f.size());
    f.render_widget(block, f.size());
}

/// Build header text.
function buildHeader(state: AppState): string {
    const filterLabel = getFilterLabel(state.filter);
    return [
        "╔════════════════════════════════════╗",
        "║       TodoX - Task Manager         ║",
        "╚════════════════════════════════════╝",
        "",
        `Filter: [${filterLabel}]  (j/k: navigate, x: toggle)`,
        "",
    ].join("\n");
}

/// Build task list text.
function buildTaskList(state: AppState, filteredTasks: Task[]): string {
    if (filteredTasks.length === 0) {
        return "  (No tasks - press 'a' to add)";
    }

    const lines: string[] = [];
    for (let i = 0; i < filteredTasks.length; i++) {
        const task = filteredTasks[i];
        const marker = task.done ? "[x]" : "[ ]";
        const prefix = i === state.selected ? "> " : "  ";
        const title = task.done ? strikethrough(task.title) : task.title;
        lines.push(`${prefix}${marker} ${title}`);
    }

    return lines.join("\n");
}

/// Build footer text.
function buildFooter(counts: { total: number; active: number; completed: number }, filter: Filter): string {
    const filterStats = getFilterStats(counts, filter);
    return [
        "",
        "────────────────────────────────────────",
        `  ${counts.total} tasks: ${counts.active} active, ${counts.completed} done`,
        `  Showing: ${filterStats}`,
        "",
        "  a: Add  d: Delete  f: Cycle filter  q: Quit",
    ].join("\n");
}

/// Get human-readable filter label.
function getFilterLabel(filter: Filter): string {
    switch (filter) {
        case Filter.All:
            return "ALL";
        case Filter.Active:
            return "ACTIVE";
        case Filter.Completed:
            return "DONE";
    }
}

/// Get filter statistics text.
function getFilterStats(counts: { active: number; completed: number }, filter: Filter): string {
    switch (filter) {
        case Filter.All:
            return "all tasks";
        case Filter.Active:
            return `${counts.active} active`;
        case Filter.Completed:
            return `${counts.completed} done`;
    }
}

/// Apply strikethrough formatting.
function strikethrough(text: string): string {
    return text.split("").join("̶");
}
