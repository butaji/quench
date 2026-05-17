import { createSignal, createEffect, For, Show } from "leptos";
import { Task } from "./models.r.ts";

export function TaskApp(): Element {
  const tasks = createSignal<Task[]>([]);
  const input = createSignal<string>("");
  const loading = createSignal<boolean>(false);

  createEffect(() => {
    loadTasks().then((data) => tasks.set(data));
  });

  const addTask = async () => {
    if (input.get().length === 0) return;
    loading.set(true);
    const task = await createTaskServer(input.get());
    const current = tasks.get();
    current.push(task);
    tasks.set(current);
    input.set("");
    loading.set(false);
  };

  const toggle = async (id: number) => {
    await toggleTaskServer(id);
    const current = tasks.get();
    const updated = current.map((t) => t.id === id ? { ...t, done: !t.done } : t);
    tasks.set(updated);
  };

  return (
    <div class="task-app">
      <Show when={() => !loading.get()} fallback={<span>Saving...</span>}>
        <div class="input-row">
          <input
            type="text"
            value={input.get()}
            oninput={(e) => input.set(e.target.value)}
          />
          <button onclick={addTask} disabled={loading.get()}>
            Add Task
          </button>
        </div>
      </Show>
      <ul class="task-list">
        <For each={tasks.get()}>
          {(task) => (
            <li
              class={task.done ? "done" : ""}
              onclick={() => toggle(task.id)}
            >
              {task.done ? "✓" : "○"} {task.title}
            </li>
          )}
        </For>
      </ul>
    </div>
  );
}
