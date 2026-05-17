import { Router, Json, State, Path } from "axum";
import { Task } from "./models.r.ts";

export type AppState = {
  tasks: Task[];
  nextId: number;
};

export function createRouter(): Router {
  return Router.new()
    .route("/tasks", { get: listTasks, post: createTask })
    .route("/tasks/:id", { get: getTask, delete: deleteTask })
    .layer(TraceLayer.new());
}

async function listTasks(
  state: State<AppState>
): Json<Task[]> {
  return Json(state.tasks);
}

async function getTask(
  state: State<AppState>,
  params: Path<{ id: number }>
): Json<Task | null> {
  for (const t of state.tasks) {
    if (t.id === params.id) return Json(t);
  }
  return Json(null);
}

async function createTask(
  state: State<AppState>,
  body: Json<{ title: string }>
): Json<Task> {
  const task: Task = {
    id: state.nextId,
    title: body.title,
    done: false,
  };
  state.tasks.push(task);
  state.nextId += 1;
  return Json(task);
}

async function deleteTask(
  state: State<AppState>,
  params: Path<{ id: number }>
): Json<boolean> {
  const initialLen = state.tasks.length;
  state.tasks = state.tasks.filter((t) => t.id !== params.id);
  return Json(state.tasks.length < initialLen);
}
