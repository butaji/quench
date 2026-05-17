import { Request, Response, Streaming } from "tonic";
import { Task } from "./models.r.ts";

export type TaskService = {
  getTask: (req: Request<{ id: number }>) => Response<Task>;
  listTasks: (req: Request<{}>) => Streaming<Task>;
  createTask: (req: Request<{ title: string }>) => Response<Task>;
};

export function taskService(): TaskService {
  return {
    getTask: handleGetTask,
    listTasks: handleListTasks,
    createTask: handleCreateTask,
  };
}

async function handleGetTask(
  req: Request<{ id: number }>
): Response<Task> {
  const task = await dbGetTask(req.id);
  if (task === null) {
    return Response.notFound("Task not found");
  }
  return Response.ok(task);
}

async function handleListTasks(
  _req: Request<{}>
): Streaming<Task> {
  const tasks = await dbListTasks();
  return Streaming.from(tasks);
}

async function handleCreateTask(
  req: Request<{ title: string }>
): Response<Task> {
  const task = await dbCreateTask(req.title);
  return Response.ok(task);
}
