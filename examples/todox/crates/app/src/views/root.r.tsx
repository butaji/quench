// root.r.tsx - Root view component

import { AppState, Task } from "../state.r.ts";
import { Block, List, ListItem, Paragraph, Sparkline, BorderType, Direction } from "rune/ui";

/**
 * Root view props.
 */
interface RootViewProps {
    tasks: Task[];
    selected: number;
    filter: string;
}

/**
 * Main root view component.
 * Transpiles to Ratatui widget construction.
 */
export function RootView(props: RootViewProps): Widget {
    return (
        <Block title="TODOX" borders={BorderType.Single}>
            <List selected={props.selected}>
                {props.tasks.map((task, i) => (
                    <ListItem
                        key={task.id}
                        selected={i === props.selected}
                    >
                        <Checkbox checked={task.done} />
                        {task.title}
                    </ListItem>
                ))}
            </List>
            <Footer tasks={props.tasks} />
        </Block>
    );
}

/**
 * Checkbox component.
 */
function Checkbox(props: { checked: boolean }): string {
    return props.checked ? "[x]" : "[ ]";
}

/**
 * Footer with task statistics.
 */
function Footer(props: { tasks: Task[] }): Paragraph {
    const done = props.tasks.filter(t => t.done).length;
    const total = props.tasks.length;
    return (
        <Paragraph>
            {done}/{total} completed
        </Paragraph>
    );
}

/**
 * Progress bar using sparkline.
 */
function ProgressBar(props: { tasks: Task[] }): Widget {
    const values = props.tasks.map(t => (t.done ? 1.0 : 0.0));
    return (
        <Sparkline
            data={values}
            direction={Direction.Horizontal}
        />
    );
}

/**
 * Task list item with actions.
 */
function TaskItem(props: { task: Task; selected: boolean }): ListItem {
    return (
        <ListItem
            selected={props.selected}
            style={props.task.done ? "strikethrough" : "normal"}
        >
            {props.task.done ? "[x] " : "[ ] "}
            {props.task.title}
        </ListItem>
    );
}
