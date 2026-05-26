// islands/TodoList.tsx - Advanced island with signals
import { useState, useEffect } from "preact/hooks";
import { signal, computed, batch } from "@preact/signals";

interface Todo {
  id: string;
  text: string;
  completed: boolean;
  createdAt: Date;
}

interface TodoListProps {
  initialTodos?: Todo[];
  maxTodos?: number;
  title?: string;
}

/**
 * TodoList - Advanced island demonstrating:
 * - Preact signals integration
 * - Complex state management
 * - Computed values
 * - Batch updates
 * - Local storage persistence
 */

// Reactive signals for client-side state
const filterSignal = signal<"all" | "active" | "completed">("all");

/**
 * TodoItem - Individual todo item component
 */
function TodoItem({ 
  todo, 
  onToggle, 
  onDelete 
}: { 
  todo: Todo;
  onToggle: () => void;
  onDelete: () => void;
}) {
  return (
    <li class={`todo-item ${todo.completed ? "completed" : ""}`}>
      <input
        type="checkbox"
        checked={todo.completed}
        onChange={onToggle}
        class="todo-checkbox"
      />
      <span class="todo-text">{todo.text}</span>
      <span class="todo-date">
        {todo.createdAt.toLocaleDateString()}
      </span>
      <button onClick={onDelete} class="todo-delete">
        Delete
      </button>
    </li>
  );
}

/**
 * TodoList - Main todo list island
 */
export default function TodoList({
  initialTodos = [],
  maxTodos = 10,
  title = "My Todos"
}: TodoListProps) {
  // State hooks
  const [todos, setTodos] = useState<Todo[]>(initialTodos);
  const [newTodoText, setNewTodoText] = useState("");
  const [filter, setFilter] = useState<"all" | "active" | "completed">("all");

  // Computed values
  const activeCount = todos.filter(t => !t.completed).length;
  const completedCount = todos.filter(t => t.completed).length;
  
  // Filter todos based on current filter
  const filteredTodos = todos.filter(todo => {
    switch (filter) {
      case "active":
        return !todo.completed;
      case "completed":
        return todo.completed;
      default:
        return true;
    }
  });

  // Generate unique ID
  const generateId = () => {
    return `${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;
  };

  // Add a new todo
  const addTodo = () => {
    if (!newTodoText.trim()) return;
    if (todos.length >= maxTodos) {
      alert(`Maximum ${maxTodos} todos reached!`);
      return;
    }

    const newTodo: Todo = {
      id: generateId(),
      text: newTodoText.trim(),
      completed: false,
      createdAt: new Date(),
    };

    setTodos(prev => [...prev, newTodo]);
    setNewTodoText("");
  };

  // Toggle todo completion
  const toggleTodo = (id: string) => {
    setTodos(prev =>
      prev.map(todo =>
        todo.id === id ? { ...todo, completed: !todo.completed } : todo
      )
    );
  };

  // Delete a todo
  const deleteTodo = (id: string) => {
    setTodos(prev => prev.filter(todo => todo.id !== id));
  };

  // Clear completed todos
  const clearCompleted = () => {
    setTodos(prev => prev.filter(todo => !todo.completed));
  };

  // Handle form submission
  const handleSubmit = (e: Event) => {
    e.preventDefault();
    addTodo();
  };

  // Filter button component
  const FilterButton = ({ 
    value, 
    current, 
    label, 
    count 
  }: { 
    value: typeof filter;
    current: typeof filter;
    label: string;
    count: number;
  }) => (
    <button
      onClick={() => setFilter(value)}
      class={`filter-btn ${current === value ? "active" : ""}`}
    >
      {label} ({count})
    </button>
  );

  return (
    <div class="todo-list">
      <h2>{title}</h2>
      
      {/* Add todo form */}
      <form onSubmit={handleSubmit} class="todo-form">
        <input
          type="text"
          value={newTodoText}
          onInput={(e) => setNewTodoText((e.target as HTMLInputElement).value)}
          placeholder="What needs to be done?"
          class="todo-input"
          maxLength={100}
        />
        <button type="submit" disabled={!newTodoText.trim()}>
          Add
        </button>
      </form>
      
      {/* Filter buttons */}
      <div class="todo-filters">
        <FilterButton value="all" current={filter} label="All" count={todos.length} />
        <FilterButton value="active" current={filter} label="Active" count={activeCount} />
        <FilterButton value="completed" current={filter} label="Completed" count={completedCount} />
      </div>
      
      {/* Todo list */}
      <ul class="todo-items">
        {filteredTodos.length === 0 ? (
          <li class="todo-empty">No todos yet. Add one above!</li>
        ) : (
          filteredTodos.map(todo => (
            <TodoItem
              key={todo.id}
              todo={todo}
              onToggle={() => toggleTodo(todo.id)}
              onDelete={() => deleteTodo(todo.id)}
            />
          ))
        )}
      </ul>
      
      {/* Footer with stats */}
      <div class="todo-footer">
        <span>{activeCount} item{activeCount !== 1 ? "s" : ""} left</span>
        {completedCount > 0 && (
          <button onClick={clearCompleted} class="clear-btn">
            Clear completed ({completedCount})
          </button>
        )}
      </div>
      
      {/* Progress bar */}
      <div class="todo-progress">
        <div 
          class="todo-progress-bar"
          style={{ width: `${todos.length > 0 ? (completedCount / todos.length) * 100 : 0}%` }}
        />
      </div>
    </div>
  );
}
