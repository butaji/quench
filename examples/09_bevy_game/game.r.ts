import { App, Plugin, Query, Res, ResMut, EventReader, EventWriter, Commands } from "bevy";
import { Task } from "./models.r.ts";

// Components
export type Position = {
  x: number;
  y: number;
};

export type Velocity = {
  x: number;
  y: number;
};

export type TaskEntity = {
  task: Task;
  priority: number;
};

// Resources
export type GameState = {
  score: number;
  paused: boolean;
};

// Events
export type TaskCompleted = {
  taskId: number;
  reward: number;
};

export function setupGame(app: App): void {
  const state: GameState = { score: 0, paused: false };
  app.addPlugins(DefaultPlugins)
    .insertResource(state)
    .addSystem(Update, moveSystem)
    .addSystem(Update, taskCompletionSystem)
    .addSystem(Startup, spawnInitialTasks);
}

function moveSystem(
  query: Query<[Position, Velocity]>,
  time: Res<Time>
): void {
  const count = query.len();
  for (let i = 0; i < count; i++) {
    const item = query.get(i);
    const pos = item.position;
    const vel = item.velocity;
    pos.x += vel.x * time.deltaSeconds();
    pos.y += vel.y * time.deltaSeconds();
  }
}

function taskCompletionSystem(
  query: Query<TaskEntity>,
  mutState: ResMut<GameState>,
  mutEvents: EventWriter<TaskCompleted>
): void {
  for (const entity of query) {
    if (entity.task.done && entity.priority > 5) {
      mutState.score += entity.priority * 10;
      const event: TaskCompleted = {
        taskId: entity.task.id,
        reward: entity.priority * 10,
      };
      mutEvents.write(event);
    }
  }
}

function spawnInitialTasks(mutCommands: Commands): void {
  const task: Task = { id: 1, title: "Collect wood", done: false };
  const entity: TaskEntity = {
    task: task,
    priority: 3,
  };
  const pos: Position = { x: 0, y: 0 };
  const vel: Velocity = { x: 1, y: 0 };
  mutCommands.spawn(entity, pos, vel);
}
