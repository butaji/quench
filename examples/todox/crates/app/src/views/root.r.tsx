//! root.r.tsx - Main view component
//!
//! Renders the main application UI using Ratatui widget patterns.
//! JSX syntax transpiles to Rust builder pattern calls.

import { AppState, Filter, Task } from "../state.r.ts";
import { task_matches_filter, get_filtered_count } from "../main.r.ts";

/// Main root view component.
export function render(state: AppState): Widget {
    const title = `TODOX - ${get_filtered_count(state)} / ${state.tasks.length}`;
    
    return (
        <Block title={title} borders="single">
            <List selected={state.selected}>
                {state.tasks.map((task, i) => (
                    task_matches_filter(task, state.filter) ? (
                        <ListItem bold={i === state.selected}>
                            {task.done ? "[x] " : "[ ] "}
                            {task.title}
                        </ListItem>
                    ) : null
                ))}
            </List>
        </Block>
    );
}

/// Widget placeholder type.
type Widget = {
    render(): string;
};

/// Block widget.
function Block(props: { title: string; borders: string }): Widget {
    return { render: () => `Block(${props.title})` };
}

/// List widget.
function List(props: { selected: number; children: Widget[] }): Widget {
    return { render: () => `List(selected=${props.selected})` };
}

/// ListItem widget.
function ListItem(props: { bold?: boolean; children: string }): Widget {
    return { render: () => `ListItem(${props.children})` };
}
