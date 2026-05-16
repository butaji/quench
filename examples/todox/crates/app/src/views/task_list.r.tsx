// task_list.r.tsx - Task list view component

import { Task } from "../state.r.ts";
import { List, ListItem, Block } from "rune/ui";

/**
 * Props for TaskList component.
 */
interface TaskListProps {
    tasks: Task[];
    selectedIndex: number;
    showCompleted: boolean;
}

/**
 * Task list component.
 */
export function TaskList(props: TaskListProps): Widget {
    const visibleTasks = props.showCompleted
        ? props.tasks
        : props.tasks.filter(t => !t.done);

    return (
        <Block title="Tasks" borders="single">
            <List selected={props.selectedIndex}>
                {visibleTasks.map((task, i) => (
                    <TaskRow
                        key={task.id}
                        task={task}
                        isSelected={i === props.selectedIndex}
                    />
                ))}
            </List>
            {visibleTasks.length === 0 && (
                <EmptyState />
            )}
        </Block>
    );
}

/**
 * Individual task row.
 */
function TaskRow(props: { task: Task; isSelected: boolean }): ListItem {
    const checkbox = props.task.done ? "[x]" : "[ ]";
    const title = props.task.done
        ? strikethrough(props.task.title)
        : props.task.title;

    return (
        <ListItem
            selected={props.isSelected}
            highlightOnSelected={true}
        >
            {checkbox} {title}
        </ListItem>
    );
}

/**
 * Empty state message.
 */
function EmptyState(): Paragraph {
    return (
        <Paragraph align="center">
            No tasks yet. Press 'a' to add one.
        </Paragraph>
    );
}

/**
 * Apply strikethrough styling.
 */
function strikethrough(text: string): string {
    return text.split("").join("\u0336");
}

/**
 * Filter tabs component.
 */
export function FilterTabs(props: {
    current: string;
    onSelect: (filter: string) => void;
}): Widget {
    const tabs = ["All", "Active", "Completed"];

    return (
        <Block borders="simple">
            <Paragraph>
                {tabs.map(tab => (
                    <TabButton
                        key={tab}
                        label={tab}
                        active={tab === props.current}
                        onClick={() => props.onSelect(tab)}
                    />
                ))}
            </Paragraph>
        </Block>
    );
}

/**
 * Tab button component.
 */
function TabButton(props: {
    label: string;
    active: boolean;
    onClick: () => void;
}): Span {
    const prefix = props.active ? "> " : "  ";
    return <Span>{prefix}{props.label}</Span>;
}
