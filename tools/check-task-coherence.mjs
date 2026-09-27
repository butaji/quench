#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const root = path.resolve(
  path.dirname(new URL(import.meta.url).pathname),
  "..",
);
const queuePath = path.join(root, "tasks", "index.json");
const queue = JSON.parse(fs.readFileSync(queuePath, "utf8"));
const items = new Map(queue.items.map((item) => [item.id, item]));
const errors = [];

if (items.size !== queue.items.length) errors.push("duplicate task ids");

const lanes = new Map();
for (const [lane, ids] of Object.entries(queue.lanes)) {
  for (const id of ids) {
    if (!items.has(id)) errors.push(`${lane} references unknown task ${id}`);
    if (lanes.has(id)) errors.push(`task ${id} appears in multiple lanes`);
    lanes.set(id, lane);
  }
}

for (const item of queue.items) {
  if (!/^\d{2}$/.test(item.id)) errors.push(`invalid task id ${item.id}`);
  if (lanes.get(item.id) !== item.lane) {
    errors.push(`lane mismatch for ${item.id}`);
  }
  if (!new Set(["pending", "in_progress", "done"]).has(item.status)) {
    errors.push(`invalid status for ${item.id}: ${item.status}`);
  }
  const file = path.join(root, "tasks", item.file);
  if (item.file !== `${item.id}.md`) {
    errors.push(`task ${item.id} must use its numbered specification file`);
  }
  if (!fs.existsSync(file)) errors.push(`missing task file ${item.file}`);
  for (const dependency of item.depends_on) {
    if (!items.has(dependency)) {
      errors.push(`${item.id} depends on unknown task ${dependency}`);
    }
    if (dependency === item.id) errors.push(`${item.id} depends on itself`);
  }
}

const active = queue.items.filter((item) => item.status === "in_progress");
const criticalPath = queue.lanes.critical_path;
const nextCriticalTask = criticalPath.find(
  (id) => items.get(id)?.status !== "done",
);
if (queue.next_task !== nextCriticalTask) {
  errors.push(
    `next_task ${queue.next_task} is not the first unfinished critical-path task (${nextCriticalTask ?? "none"})`,
  );
}
if (queue.next_task !== null && items.get(queue.next_task)?.status !== "in_progress") {
  errors.push(`next_task ${queue.next_task} is not in progress`);
}

const visiting = new Set();
const visited = new Set();
function visit(id) {
  if (visiting.has(id)) errors.push(`dependency cycle at ${id}`);
  if (visited.has(id)) return;
  visiting.add(id);
  for (const dependency of items.get(id)?.depends_on ?? []) {
    if (items.has(dependency)) visit(dependency);
  }
  visiting.delete(id);
  visited.add(id);
}
for (const id of items.keys()) visit(id);

if (errors.length) {
  console.error(errors.map((error) => `error: ${error}`).join("\n"));
  process.exit(1);
}
console.log(
  `task queue coherent: ${items.size} tasks, ${active.length} active, next=${queue.next_task}`,
);
