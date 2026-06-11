// Todo List Example - Quench demo (TypeScript)
// Demonstrates nested layouts, keyboard navigation, and state management

import { render, Box, Text, useState, useInput, useApp } from 'ink';

interface TodoItem {
  id: number;
  text: string;
  done: boolean;
}

function TodoList(): JSX.Element {
  const [todos, setTodos] = useState<TodoItem[]>([
    { id: 1, text: 'Learn Quench', done: true },
    { id: 2, text: 'Build a TUI app', done: false },
    { id: 3, text: 'Ship it!', done: false },
  ]);
  const [selected, setSelected] = useState(0);

  useInput((input: string) => {
    if (input === 'j' || input === 'downArrow') {
      setSelected((s: number) => Math.min(s + 1, todos.length - 1));
    }
    if (input === 'k' || input === 'upArrow') {
      setSelected((s: number) => Math.max(s - 1, 0));
    }
    if (input === ' ') {
      setTodos((items: TodoItem[]) =>
        items.map((t: TodoItem, i: number) =>
          i === selected ? { ...t, done: !t.done } : t
        )
      );
    }
    if (input === 'q' || input === 'Q') {
      useApp().exit();
    }
  });

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="green">Todo List</Text>
      <Box flexDirection="column" marginTop={1}>
        {todos.map((todo: TodoItem, index: number) => (
          <Box key={todo.id}>
            <Text>
              {index === selected ? '> ' : '  '}
              {todo.done ? '[x] ' : '[ ] '}
              {todo.text}
            </Text>
          </Box>
        ))}
      </Box>
      <Text dimColor marginTop={1}>
        [j/k] move | [space] toggle | [q] quit
      </Text>
    </Box>
  );
}

render(<TodoList />);
