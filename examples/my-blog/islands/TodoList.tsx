// islands/TodoList.tsx - Todo list island with hooks
import { useState } from "preact/hooks";

interface Todo {
  id: string;
  text: string;
  completed: boolean;
}

interface TodoListProps {
  title?: string;
}

export default function TodoList({ title = "My Todos" }: TodoListProps) {
  const [todos, setTodos] = useState<Todo[]>([]);
  const [newTodo, setNewTodo] = useState("");

  function handleAdd() {
    if (newTodo.trim().length === 0) return;

    const todo: Todo = {
      id: "todo-1",
      text: newTodo.toString(),
      completed: false,
    };

    setTodos([...todos, todo]);
    setNewTodo("");
  }

  function handleToggle(id: string) {
    const updated: Todo[] = todos.map(t => {
      const item: Todo = { id: t.id, text: t.text, completed: t.completed };
      if (t.id === id) {
        item.completed = !t.completed;
      }
      return item;
    });
    setTodos(updated);
  }

  function handleDelete(id: string) {
    setTodos(todos.filter(t => t.id !== id));
  }

  const activeCount = todos.filter(t => !t.completed).length;

  return (
    <div class="todo-list">
      <h3>{title}</h3>

      <div class="todo-input-row">
        <input
          type="text"
          value={newTodo}
          onInput={(e) => setNewTodo(e.currentTarget.value)}
          placeholder="What needs to be done?"
        />
        <button onClick={handleAdd}>Add</button>
      </div>

      <ul class="todo-items">
        {todos.length === 0 && (
          <li class="empty">No todos yet</li>
        )}
        {todos.map((todo, i) => (
          <li key={i} class={todo.completed ? "completed" : ""}>
            <input
              type="checkbox"
              checked={todo.completed}
              onChange={() => handleToggle(todo.id)}
            />
            <span>{todo.text}</span>
            <button onClick={() => handleDelete(todo.id)}>Delete</button>
          </li>
        ))}
      </ul>

      <p class="count">{activeCount} items left</p>
    </div>
  );
}
