import { State, Window, Emitter } from "tauri";
import { Task } from "./models.r.ts";

export type AppState = {
  tasks: Task[];
  nextId: number;
};

export function initState(): AppState {
  return { tasks: [], nextId: 1 };
}

export function getTasks(state: State<AppState>): Task[] {
  return state.tasks;
}

export function addTask(
  state: State<AppState>,
  title: string
): { ok: true; task: Task } | { ok: false; error: string } {
  if (title.length === 0) {
    return { ok: false, error: "Title required" };
  }
  const task: Task = {
    id: state.nextId,
    title,
    done: false,
  };
  state.tasks.push(task);
  state.nextId += 1;
  return { ok: true, task };
}

export function toggleTask(
  state: State<AppState>,
  id: number
): boolean {
  for (const t of state.tasks) {
    if (t.id === id) {
      t.done = !t.done;
      return true;
    }
  }
  return false;
}

export function emitUpdate(window: Window, tasks: Task[]): void {
  window.emit("tasks:updated", tasks);
}
