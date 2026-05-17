import { Pool, Postgres, queryAs } from "sqlx";
import { Task } from "./models.r.ts";

export type DbPool = Pool<Postgres>;

export async function initDb(databaseUrl: string): DbPool {
  return Pool.connect(databaseUrl);
}

export async function getTaskById(pool: DbPool, id: number): Task | null {
  const rows = await queryAs<Task>(
    pool,
    "SELECT id, title, done FROM tasks WHERE id = $1",
    [id]
  );
  return rows.length > 0 ? rows[0] : null;
}

export async function createTask(pool: DbPool, title: string): Task {
  const rows = await queryAs<Task>(
    pool,
    "INSERT INTO tasks (title, done) VALUES ($1, false) RETURNING id, title, done",
    [title]
  );
  return rows[0];
}

export async function listTasks(pool: DbPool): Task[] {
  return await queryAs<Task>(
    pool,
    "SELECT id, title, done FROM tasks ORDER BY id"
  );
}
