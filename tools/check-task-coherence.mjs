#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { createHash } from "node:crypto";

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

const frameworkManifest = "crates/quench-node-test/node-tests/parallel.txt";
const frameworkTestRoot = "tests/node/test/parallel";
const frameworkManifestBytes = fs.readFileSync(path.join(root, frameworkManifest));
const frameworkManifestText = frameworkManifestBytes.toString("utf8");
const frameworkLines = frameworkManifestText.split("\n");
const manifestNames = new Set();
const frameworkFixtures = [];
for (const [index, line] of frameworkLines.entries()) {
  const [namePart, annotation = ""] = line.split("#", 2);
  const name = namePart.trim();
  if (!name) continue;
  if (manifestNames.has(name)) {
    errors.push(`duplicate Node manifest member at line ${index + 1}: ${name}`);
  }
  manifestNames.add(name);
  if (
    path.isAbsolute(name) ||
    name.split("/").some((part) => part === ".." || part === "." || !part)
  ) {
    errors.push(`invalid Node manifest member: ${name}`);
    continue;
  }
  const tags = [...annotation.matchAll(/(?:^|\s)profile=([^\s]+)/g)]
    .map((match) => match[1]);
  if (new Set(tags).size !== tags.length) {
    errors.push(`duplicate profile tag for Node manifest member: ${name}`);
  }
  for (const tag of tags) {
    if (!/^[a-z][a-z0-9-]*$/.test(tag)) {
      errors.push(`invalid profile tag ${tag}: ${name}`);
    }
  }
  if (annotation.includes("profile=") && !tags.length) {
    errors.push(`malformed profile tag for Node manifest member: ${name}`);
  }
  const fixture = path.posix.join(frameworkTestRoot, name);
  if (!fs.existsSync(path.join(root, fixture))) {
    errors.push(`missing Node manifest fixture ${fixture}`);
  }
  if (tags.includes("framework-core")) frameworkFixtures.push(fixture);
}
if (frameworkFixtures.length === 0) {
  errors.push(`empty framework-core profile in ${frameworkManifest}`);
}

const nodeEvidencePath = "tasks/evidence/task86-stagea-final-2026-10-08.json";
const nodeEvidence = JSON.parse(
  fs.readFileSync(path.join(root, nodeEvidencePath), "utf8"),
);
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const fileSha256 = (relativePath) =>
  sha256(fs.readFileSync(path.join(root, relativePath)));
const recordedCases = nodeEvidence.authority?.cases ?? [];
const recordedFixturePaths = recordedCases.map((item) => item.path);
if (nodeEvidence.authority?.node_parallel_manifest !== frameworkManifest) {
  errors.push(`Node evidence points at a different manifest: ${nodeEvidencePath}`);
}
if (nodeEvidence.authority?.profile !== "framework-core") {
  errors.push(`Node evidence has the wrong profile: ${nodeEvidencePath}`);
}
if (
  nodeEvidence.authority?.node_parallel_manifest_sha256 !==
  sha256(frameworkManifestBytes)
) {
  errors.push(`changed Node profile manifest: ${frameworkManifest}`);
}
if (
  nodeEvidence.authority?.cases_selected_from_manifest !== frameworkFixtures.length ||
  recordedFixturePaths.length !== frameworkFixtures.length ||
  recordedFixturePaths.some((fixture, index) => fixture !== frameworkFixtures[index])
) {
  errors.push(`Node qualification evidence does not match ${frameworkManifest}`);
}
for (const item of recordedCases) {
  if (item.quench_status !== "pass") {
    errors.push(`Node framework fixture did not pass: ${item.path}`);
  }
  if (!fs.existsSync(path.join(root, item.path))) {
    errors.push(`missing Node evidence fixture ${item.path}`);
  } else if (item.sha256 !== fileSha256(item.path)) {
    errors.push(`changed Node framework fixture ${item.path}`);
  }
}
if (
  nodeEvidence.profile_result?.total !== frameworkFixtures.length ||
  nodeEvidence.profile_result?.pass !== frameworkFixtures.length ||
  nodeEvidence.profile_result?.skip !== 0 ||
  nodeEvidence.profile_result?.fail !== 0 ||
  nodeEvidence.profile_result?.timeout !== 0 ||
  nodeEvidence.profile_result?.crash !== 0 ||
  nodeEvidence.profile_result?.unclassified !== 0
) {
  errors.push(`Node framework qualification is not all-pass: ${nodeEvidencePath}`);
}

const frameworkReadme = fs.readFileSync(
  path.join(root, "tests/frameworks/README.md"),
  "utf8",
);
const scenarioRows = [...frameworkReadme.matchAll(
  /^\|\s*`scenarios\/([^`]+)`\s*\|\s*([^|]+)\|\s*$/gm,
)];
const scenarioNames = new Set();
for (const [, file, packageName] of scenarioRows) {
  if (scenarioNames.has(file)) errors.push(`duplicate framework scenario ${file}`);
  scenarioNames.add(file);
  if (!fs.existsSync(path.join(root, "tests/frameworks/scenarios", file))) {
    errors.push(`missing framework scenario ${file}`);
  }
  if (!packageName.trim()) errors.push(`missing package for framework scenario ${file}`);
}
if (scenarioRows.length === 0) errors.push("no pinned framework scenarios listed");
if (!fs.existsSync(path.join(root, "tests/frameworks/package-lock.json"))) {
  errors.push("missing pinned framework package lock");
}
const scenarioInputHashes = nodeEvidence.authority?.framework_scenario_inputs_sha256 ?? {};
const expectedScenarioInputs = [
  "tests/frameworks/driver.cjs",
  ...[...scenarioNames].map((file) => `tests/frameworks/scenarios/${file}`),
  "tests/frameworks/scenarios/asset.txt",
  "tests/frameworks/package.json",
  "tests/frameworks/package-lock.json",
].sort();
const recordedScenarioInputs = Object.keys(scenarioInputHashes).sort();
if (
  expectedScenarioInputs.length !== recordedScenarioInputs.length ||
  expectedScenarioInputs.some((file, index) => file !== recordedScenarioInputs[index])
) {
  errors.push(`Node scenario evidence does not match ${nodeEvidencePath}`);
}
for (const [file, expectedHash] of Object.entries(scenarioInputHashes)) {
  if (!fs.existsSync(path.join(root, file))) {
    errors.push(`missing Node scenario input ${file}`);
  } else if (expectedHash !== fileSha256(file)) {
    errors.push(`changed Node scenario input ${file}`);
  }
}
for (const [name, result] of Object.entries(nodeEvidence.scenarios ?? {})) {
  if (
    result.timeout ||
    result.exit_status?.node !== 0 ||
    result.exit_status?.quench !== 0 ||
    result.exit_equal !== true ||
    result.stdout_equal !== true ||
    result.stderr_equal !== true
  ) {
    errors.push(`Node framework scenario is not an exact successful match: ${name}`);
  }
}

if (errors.length) {
  console.error(errors.map((error) => `error: ${error}`).join("\n"));
  process.exit(1);
}
console.log(
  `task queue coherent: ${items.size} tasks, ${active.length} active, next=${queue.next_task}, framework-core fixtures=${frameworkFixtures.length}, package scenarios=${scenarioRows.length}`,
);
