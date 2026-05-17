// views/task_list.r.tsx - Task list component.
// Demonstrates: TSX component patterns, Ratatui List widget

import { Task, Filter, filterTasks } from "../state.r.ts";

/// Props for the task list component.
export type TaskListProps = {
    tasks: Task[];
    selected: number;
    filter: Filter;
    onSelect: (index: number) => void;
};

/// Task list component - renders a list of tasks with selection.
export function TaskList(props: TaskListProps): Widget {
    const filtered = filterTasks(props.tasks, props.filter);
    
    // Build list items
    const items: ListItem[] = [];
    for (let i = 0; i < filtered.length; i++) {
        const task = filtered[i];
        const text = task.done ? "[x] " + task.title : "[ ] " + task.title;
        const style = i === props.selected 
            ? Style.new().fg(Color.Yellow) 
            : Style.default();
        items.push(ListItem.new(text).style(style));
    }
    
    // Create list widget using Ratatui builder pattern
    const list = List.new(items)
        .block(Block.default().title("Tasks").borders(Borders.SINGLE))
        .highlight_style(Style.new().add_modifier(Modifier.Reverse));
    
    return list;
}
