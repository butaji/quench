import { App, HttpServer, web, HttpResponse } from "actix_web";
import { Task } from "./models.r.ts";

export type AppState = {
  tasks: Task[];
};

export function createApp(): App {
  return App.new()
    .appData(web.Data({ tasks: [] }))
    .service(
      web.scope("/api")
        .route("/tasks", { get: listTasks })
        .route("/tasks", { post: createTask })
    );
}

async function listTasks(data: web.Data<AppState>): HttpResponse {
  return HttpResponse.Ok().json(data.tasks);
}

async function createTask(
  data: web.Data<AppState>,
  body: web.Json<{ title: string }>
): HttpResponse {
  const task: Task = {
    id: data.tasks.length + 1,
    title: body.title,
    done: false,
  };
  data.tasks.push(task);
  return HttpResponse.Created().json(task);
}
