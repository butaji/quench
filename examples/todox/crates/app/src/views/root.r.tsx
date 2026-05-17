// root.r.tsx - Main view component
// Transpiles JSX to Ratatui widget construction

import { Task, Filter } from "../state.r.ts";

interface RootViewProps {
  tasks: Task[];
  selected: usize;
  filter: Filter;
}

/**
 * Main application view - renders the task list.
 * JSX transpiles to Ratatui builder patterns.
 */
export function renderRootView(props: RootViewProps): Widget {
  const { tasks, selected, filter } = props;
  const visibleTasks = filterTasks(tasks, filter);
  const title = getTitle(filter);

  return (
    <Block title={title} borders="single">
      <List selected={selected}>
        {visibleTasks.map((task, i) => (
          <ListItem
            bold={i === selected}
            fg={task.done ? "green" : "white"}
          >
            {task.done ? "[x] " : "[ ] "}
            {task.title}
          </ListItem>
        ))}
      </List>
      <Paragraph text={`Tasks: ${visibleTasks.length}`} />
    </Block>
  );
}

/**
 * Get title based on filter mode.
 */
function getTitle(filter: Filter): string {
  switch (filter) {
    case Filter.All:
      return "TODOX - All Tasks";
    case Filter.Active:
      return "TODOX - Active Tasks";
    case Filter.Completed:
      return "TODOX - Completed Tasks";
  }
}

/**
 * Render footer with task counts.
 */
export function renderFooter(tasks: Task[]): Widget {
  const total = tasks.length;
  const completed = tasks.filter(t => t.done).length;
  const active = total - completed;

  return (
    <Block title="Stats" borders="bottom">
      <Text>
        Total: {total} | Active: {active} | Done: {completed}
      </Text>
    </Block>
  );
}

// Import Ratatui widget types
// These are re-exported from protocol via the wiring layer
import type { Widget } from "protocol";
