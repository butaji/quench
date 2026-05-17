import { useSignal, useMemo } from "dioxus";
import { Task } from "./models.r.ts";

interface AppProps {
  initialTasks: Task[];
}

export function App(props: AppProps): Element {
  const tasks = useSignal<Task[]>(props.initialTasks);
  const input = useSignal<string>("");
  const filter = useSignal<string>("all");

  const filtered = useMemo(() => {
    return tasks().filter((t) => {
      if (filter() === "active") return !t.done;
      if (filter() === "done") return t.done;
      return true;
    });
  });

  const addTask = () => {
    if (input().length === 0) return;
    const newTask: Task = {
      id: tasks().length + 1,
      title: input(),
      done: false,
    };
    const current = tasks();
    current.push(newTask);
    tasks.set(current);
    input.set("");
  };

  const toggle = (id: number) => {
    tasks.set(tasks().map((t) => t.id === id ? { ...t, done: !t.done } : t));
  };

  return (
    <div className="app">
      <div className="filters">
        {["all", "active", "done"].map((f) => (
          <button
            className={filter() === f ? "active" : ""}
            onclick={() => filter.set(f)}
          >
            {f}
          </button>
        ))}
      </div>
      <div className="input-row">
        <input value={input()} oninput={(e) => input.set(e.value)} />
        <button onclick={addTask}>Add</button>
      </div>
      <ul className="task-list">
        {filtered().map((task) => (
          <li
            className={task.done ? "done" : ""}
            onclick={() => toggle(task.id)}
          >
            {task.done ? "[x]" : "[ ]"} {task.title}
          </li>
        ))}
      </ul>
    </div>
  );
}
