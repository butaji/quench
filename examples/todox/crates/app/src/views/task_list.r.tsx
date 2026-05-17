// task_list.r.tsx - Task list component
// Demonstrates conditional rendering and array methods

import { Task } from "../state.r.ts";

/**
 * Task item row with toggle capability.
 */
export function TaskRow(props: {
  task: Task;
  index: number;
  selected: boolean;
}): Widget {
  const { task, index, selected } = props;
  const prefix = task.done ? "[x]" : "[ ]";
  const style = selected ? "bold" : "normal";

  return (
    <ListItem
      index={index}
      selected={selected}
      style={style}
    >
      {prefix} {task.title}
    </ListItem>
  );
}

/**
 * Render a list of tasks with selection highlighting.
 */
export function renderTaskList(
  tasks: Task[],
  selected: number
): Widget[] {
  return tasks.map((task, i) => (
    <TaskRow
      task={task}
      index={i}
      selected={i === selected}
    />
  ));
}

/**
 * Empty state when no tasks exist.
 */
export function renderEmptyState(message: string): Widget {
  return (
    <Block title="Tasks" borders="single">
      <Paragraph text={message} align="center" />
    </Block>
  );
}

/**
 * Batch task operations view.
 */
export function renderBatchActions(): Widget {
  return (
    <Block title="Actions" borders="single">
      <Text>
        [a] Add task | [d] Delete | [t] Toggle | [q] Quit
      </Text>
    </Block>
  );
}

// Widget type from Ratatui
import type { Widget } from "protocol";
