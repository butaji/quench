#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";

const root = path.resolve(
  path.dirname(new URL(import.meta.url).pathname),
  "..",
);
const queuePath = path.join(root, "tasks", "index.json");
const queue = JSON.parse(fs.readFileSync(queuePath, "utf8"));
const items = new Map(queue.items.map((item) => [item.id, item]));
const errors = [];

if (queue.schema !== 3) errors.push(`unsupported queue schema ${queue.schema}`);
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
  if (!lanes.has(item.id)) errors.push(`task ${item.id} has no lane`);
  if (Object.hasOwn(item, "lane")) {
    errors.push(`task ${item.id} duplicates lane membership`);
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
    if (
      item.status !== "pending" && items.get(dependency)?.status !== "done"
    ) {
      errors.push(
        `${item.id} is ${item.status} before prerequisite ${dependency}`,
      );
    }
  }
}

const active = queue.items.filter((item) => item.status === "in_progress");
const criticalPath = queue.lanes.critical_path;
const nextCriticalTask = criticalPath.find(
  (id) => items.get(id)?.status !== "done",
) ?? null;
if (queue.next_task !== nextCriticalTask) {
  errors.push(
    `next_task ${queue.next_task} is not the first unfinished critical-path task (${
      nextCriticalTask ?? "none"
    })`,
  );
}
if (
  queue.next_task !== null &&
  items.get(queue.next_task)?.status !== "in_progress"
) {
  errors.push(`next_task ${queue.next_task} is not in progress`);
}

const visiting = new Set();
const visited = new Set();
function visit(id) {
  if (visiting.has(id)) {
    errors.push(`dependency cycle at ${id}`);
    return;
  }
  if (visited.has(id)) return;
  visiting.add(id);
  for (const dependency of items.get(id)?.depends_on ?? []) {
    if (items.has(dependency)) visit(dependency);
  }
  visiting.delete(id);
  visited.add(id);
}
for (const id of items.keys()) visit(id);

const stageOf = new Map();
for (const [position, name] of queue.phases.order.entries()) {
  const stage = queue.phases[name];
  if (!stage) {
    errors.push(`unknown stage ${name}`);
    continue;
  }
  if (!stage.tasks.includes(stage.gate) || !criticalPath.includes(stage.gate)) {
    errors.push(
      `${name} gate ${stage.gate} must belong to its stage and critical path`,
    );
  }
  for (const id of stage.tasks) {
    if (!items.has(id)) errors.push(`${name} references unknown task ${id}`);
    if (stageOf.has(id)) errors.push(`task ${id} appears in multiple stages`);
    if (lanes.get(id) === "deferred") {
      errors.push(`deferred task ${id} belongs to ${name}`);
    }
    stageOf.set(id, position);
  }
}

function dependsOn(id, target, seen = new Set()) {
  if (seen.has(id)) return false;
  seen.add(id);
  return (items.get(id)?.depends_on ?? []).some(
    (dependency) =>
      dependency === target || dependsOn(dependency, target, seen),
  );
}

for (const item of queue.items) {
  if (lanes.get(item.id) === "deferred") continue;
  const position = stageOf.get(item.id);
  if (position === undefined) {
    errors.push(`active-plan task ${item.id} has no stage`);
    continue;
  }
  for (const dependency of item.depends_on) {
    if (
      lanes.get(dependency) === "deferred" || stageOf.get(dependency) > position
    ) {
      errors.push(
        `${item.id} depends on work outside or after its stage: ${dependency}`,
      );
    }
  }
  if (position > 0) {
    const priorGate = queue.phases[queue.phases.order[position - 1]]?.gate;
    if (!dependsOn(item.id, priorGate)) {
      errors.push(`${item.id} can start before prior stage gate ${priorGate}`);
    }
  }
}

const nodeInventory = JSON.parse(
  fs.readFileSync(path.join(root, "tasks/node-compat-inventory.json"), "utf8"),
);
const nodeEntries = new Map();
const caseRoles = new Set(["assertion_case", "observation_case"]);
const supportRoles = new Set(["module", "package_metadata", "child_process"]);
const nonExecutableRoles = new Set(["documentation", "inventory_manifest"]);
for (const entry of nodeInventory.entries) {
  if (nodeEntries.has(entry.path)) {
    errors.push(`duplicate Node input ${entry.path}`);
  }
  nodeEntries.set(entry.path, entry);
  if (
    !caseRoles.has(entry.role) && !supportRoles.has(entry.role) &&
    !nonExecutableRoles.has(entry.role)
  ) {
    errors.push(`unknown Node input role ${entry.role}: ${entry.path}`);
  }
  const inputPath = path.join(root, entry.path);
  if (!fs.existsSync(inputPath)) {
    errors.push(`missing Node input ${entry.path}`);
    continue;
  }
  const digest = createHash("sha256").update(fs.readFileSync(inputPath)).digest(
    "hex",
  );
  if (digest !== entry.sha256) errors.push(`changed Node input ${entry.path}`);
}
const declaredNodeInputs = [];
for (const source of nodeInventory.sources) {
  if (source.kind === "tracked_inputs") {
    declaredNodeInputs.push(
      ...execFileSync(
        "git",
        ["ls-files", "--", source.root],
        { cwd: root, encoding: "utf8" },
      ).trim().split("\n").filter(Boolean),
    );
  } else if (source.kind === "manifest") {
    if (nodeEntries.get(source.manifest)?.role !== "inventory_manifest") {
      errors.push(
        `Node source manifest is not inventoried: ${source.manifest}`,
      );
    }
    const manifestLines = fs.readFileSync(
      path.join(root, source.manifest),
      "utf8",
    )
      .split("\n");
    const manifestEntries = manifestLines.flatMap((line) => {
      const [namePart, annotation = ""] = line.split("#", 2);
      const name = namePart.trim();
      if (!name) return [];
      const tagged = [...annotation.matchAll(/(?:^|\s)profile=([^\s]+)/g)]
        .map((match) => match[1]);
      if (new Set(tagged).size !== tagged.length) {
        errors.push(`duplicate profile tag for Node manifest member: ${name}`);
      }
      for (const profile of tagged) {
        if (!/^[a-z][a-z0-9-]*$/.test(profile)) {
          errors.push(`invalid profile tag ${profile}: ${name}`);
        }
      }
      if (annotation.includes("profile=") && !tagged.length) {
        errors.push(`malformed profile tag for Node manifest member: ${name}`);
      }
      return [{ name, profiles: tagged }];
    });
    const names = manifestEntries.map((entry) => entry.name);
    if (!names.length) errors.push(`empty Node manifest: ${source.manifest}`);
    if (new Set(names).size !== names.length) {
      errors.push(`duplicate Node manifest member: ${source.manifest}`);
    }
    for (const name of names) {
      if (
        path.isAbsolute(name) ||
        name.split("/").some((part) => part === ".." || part === "." || !part)
      ) {
        errors.push(`invalid Node manifest member: ${name}`);
      }
      const fixture = path.posix.join(source.root, name);
      declaredNodeInputs.push(fixture);
      if (!caseRoles.has(nodeEntries.get(fixture)?.role)) {
        errors.push(`Node manifest member is not a case: ${fixture}`);
      }
    }
  } else {
    errors.push(`unknown Node input source: ${source.kind}`);
  }
}
if (
  JSON.stringify(declaredNodeInputs) !== JSON.stringify([...nodeEntries.keys()])
) {
  errors.push(
    "Node inventory must contain all declared inputs in discovery order",
  );
}
const nodeRevision = execFileSync(
  "git",
  ["rev-parse", "HEAD"],
  { cwd: path.join(root, nodeInventory.upstream.root), encoding: "utf8" },
).trim();
if (nodeRevision !== nodeInventory.upstream.revision) {
  errors.push("Node inventory upstream revision changed");
}
for (const entry of nodeEntries.values()) {
  if (!supportRoles.has(entry.role)) continue;
  if (!entry.consumers?.length) {
    errors.push(`Node support input has no consumer: ${entry.path}`);
  }
  for (const consumer of entry.consumers ?? []) {
    if (!caseRoles.has(nodeEntries.get(consumer)?.role)) {
      errors.push(
        `Node support ${entry.path} has unknown/non-case consumer ${consumer}`,
      );
    }
  }
}

if (errors.length) {
  console.error(errors.map((error) => `error: ${error}`).join("\n"));
  process.exit(1);
}
console.log(
  `task queue coherent: ${items.size} tasks, ${active.length} active, next=${queue.next_task}, Node inputs=${nodeEntries.size}`,
);
