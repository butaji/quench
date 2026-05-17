import { useState, useEffect, functionComponent } from "yew";
import { Task } from "./models.r.ts";

export function TaskList(): Element {
  const tasks = useState<Task[]>([]);
  const draft = useState<string>("");

  useEffect(() => {
    fetchTasks().then((data) => tasks.set(data));
  }, []);

  const addTask = () => {
    if (draft.value.length === 0) return;
    const task: Task = {
      id: tasks.value.length + 1,
      title: draft.value,
      done: false,
    };
    tasks.value.push(task);
    tasks.set(tasks.value);
    draft.set("");
  };

  const toggle = (id: number) => {
    const updated = tasks.value.map((t) =>
      t.id === id ? { ...t, done: !t.done } : t
    );
    tasks.set(updated);
  };

  return (
    <div className="task-list">
      <div className="controls">
        <input
          type="text"
          value={draft.value}
          oninput={(e) => draft.set(e.target.value)}
        />
        <button onclick={addTask}>Add</button>
      </div>
      <ul>
        {tasks.value.map((task) => (
          <li
            key={task.id}
            className={task.done ? "completed" : ""}
            onclick={() => toggle(task.id)}
          >
            <input type="checkbox" checked={task.done} />
            <span>{task.title}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
