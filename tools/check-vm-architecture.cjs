#!/usr/bin/env node
"use strict";

// Read-only architecture gate for the interpreter migration. It checks the
// declarations and build boundaries; it never executes a guest or benchmark.
const fs = require("fs");
const path = require("path");

const root = path.resolve(process.argv[2] || path.join(__dirname, ".."));
const failures = [];
const read = (relative) => fs.readFileSync(path.join(root, relative), "utf8");
const fail = (message) => failures.push(message);

const goal = read("GOAL.md");
if (!goal.includes("declarative Rust macro")) fail("GOAL must require declarative macros");
if (!goal.includes("There is no JIT")) fail("GOAL must remain interpreter-only");
if (!goal.includes("Workload-specific kernels and their fact")) {
  fail("GOAL must prohibit workload-specific fact recognizers");
}
if (!goal.includes("Reusable\nkernels are allowed")) {
  fail("GOAL must preserve reusable fact-guarded kernels");
}

const ir = read("crates/quench-runtime/src/ir.rs");
if ((ir.match(/vm_op!\s*\{/g) || []).length !== 1) {
  fail("ir.rs must have one vm_op! operation declaration");
}
if (ir.includes("instruction_set!")) fail("legacy instruction_set! declaration remains");
if (!ir.includes("CompactInstructionBuilder")) {
  fail("operation catalog must expose the compact instruction builder");
}
if (!ir.includes("decode_operands")) {
  fail("operation catalog must expose generated operand decoding");
}
for (const view of ["result_shape", "control_flow", "control_operands", "guards"]) {
  if (!ir.includes(view)) fail(`operation catalog must expose generated ${view}`);
}
if (!ir.includes("CompactHandler") || !ir.includes("handler(self)")) {
  fail("operation catalog must expose generated opcode handlers");
}
if (!ir.includes("ControlOperands") || !ir.includes("ControlFlow")) {
  fail("operation catalog must expose generated control transitions");
}

// The macro invocation is the single source of opcode facts.  Keep a cheap
// textual check here so a hand-edited generated view cannot silently drift
// from the declaration without the Rust build being involved.
const catalogRows = [...ir.matchAll(/^\s*([A-Za-z][A-Za-z0-9_]*)\s*=\s*(\d+)\s*\/\s*\d+\s*=>\s*\[[^\]]*\]\s*\/\s*[A-Za-z][A-Za-z0-9_]*\s*\/\s*[A-Za-z][A-Za-z0-9_]*\s*\/\s*[A-Za-z][A-Za-z0-9_]*\s*\/\s*\[[^\]]*\]\s*\/\s*([A-Za-z][A-Za-z0-9_]*)/gm)];
if (catalogRows.length === 0) fail("operation catalog has no vm_op rows");
for (let index = 0; index < catalogRows.length; index += 1) {
  const id = Number(catalogRows[index][2]);
  if (id !== index + 1) fail(`operation catalog IDs must be contiguous (row ${index + 1})`);
}
const operationCount = ir.match(/pub const COUNT: u8 = vm_op!\(@last/) ? catalogRows.length : 0;
if (operationCount !== catalogRows.length) fail("generated operation count is not tied to catalog rows");
for (const [, opcode, , handler] of catalogRows) {
  if (!new RegExp(`(?:fn|pub(?:\\([^)]*\\))?\\s+fn)\\s+${handler}\\b`).test(
    read("crates/quench-runtime/src/vm/vm_runtime.rs") + read("crates/quench-runtime/src/vm/vm_dispatch.rs")
  )) {
    fail(`catalog handler ${opcode} has no runtime implementation: ${handler}`);
  }
}
const quickening = read("crates/quench-runtime/src/quickening.rs");
if (!quickening.includes("QuickeningSite") || !quickening.includes("QuickeningDecision")) {
  fail("quickening must expose bounded site decisions");
}
const runtime = read("crates/quench-runtime/src/vm/vm_runtime.rs");
if (!runtime.includes("struct DispatchTransition") || !runtime.includes("next_pc")) {
  fail("dispatch boundary must carry an explicit next-pc transition");
}
const coldFallbacks = ["run_slow_fallback", "run_compact_call_fallback", "run_compact_get_property_fallback", "run_compact_get_named_fallback", "run_compact_set_index_fallback", "run_compact_get_index_fallback", "run_compact_get_index_inc_fallback"];
for (const helper of coldFallbacks) {
  const marker = new RegExp(`#\\[cold\\][\\s\\S]{0,80}fn\\s+${helper}\\b`);
  if (!marker.test(runtime)) fail(`${helper} must remain an outlined cold fallback`);
}
const dynamicShape = read("crates/quench-runtime/src/dynamic/shape.rs");
if (dynamicShape.includes("pub struct ShapeId")) {
  fail("dynamic adapter must reuse canonical identity::ShapeId");
}
if (!read("crates/quench-runtime/src/facts.rs").includes("SharedBinaryFact")) {
  fail("shared JS/Wasm semantic facts must remain declared in facts.rs");
}

const tasks = JSON.parse(read("tasks/index.json"));
if (tasks.schema !== 1 || !Array.isArray(tasks.items) || tasks.items.length === 0) {
  fail("tasks/index.json has an invalid schema or empty queue");
} else {
  const ids = new Set(tasks.items.map((item) => item.id));
  if (ids.size !== tasks.items.length) fail("task IDs must be unique");
  for (const item of tasks.items) {
    for (const dependency of item.depends_on || []) {
      if (!ids.has(dependency)) fail(`${item.id} depends on missing task ${dependency}`);
    }
  }
}

const cargo = read("crates/quench-runtime/Cargo.toml");
if (cargo.includes("benchmark-kernels")) {
  fail("benchmark-kernels feature must not exist in quench-runtime");
}
const defaultFeatures = cargo.match(/^default\s*=\s*\[([^\]]*)\]/m);
if (defaultFeatures && /execution-trace|benchmark|profile/i.test(defaultFeatures[1])) {
  fail("diagnostic or benchmark features must not leak into the default runtime");
}

// Production execution must not acquire workload identity as a hidden fact.
// Keep this scan limited to runtime sources; benchmark harnesses are allowed
// to name their fixtures because they never ship in the VM.
const runtimeRoot = path.join(root, "crates", "quench-runtime", "src");
const forbiddenRuntimeTokens = [/richards/i, /deltablue/i, /navier[-_ ]?stokes/i,
  /earley[-_ ]?boyer/i, /fixture\s*name/i, /suite\s*marker/i, /benchmark\s*score/i];
function walk(directory) {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const target = path.join(directory, entry.name);
    if (entry.isDirectory()) walk(target);
    else if (entry.isFile() && /\.(rs|toml)$/.test(entry.name)) {
      const source = fs.readFileSync(target, "utf8");
      for (const token of forbiddenRuntimeTokens) {
        if (token.test(source)) fail(`forbidden workload token in runtime source: ${path.relative(root, target)}`);
      }
    }
  }
  const taskDirectory = path.join(root, "tasks");
  const indexedFiles = new Set(tasks.items.map((item) => item.file));
  const taskFiles = fs.readdirSync(taskDirectory)
    .filter((file) => file.endsWith(".md"));
  for (const file of taskFiles) {
    if (!indexedFiles.has(file)) fail(`task file is not indexed: tasks/${file}`);
  }
  for (const file of indexedFiles) {
    if (!fs.existsSync(path.join(taskDirectory, file))) {
      fail(`indexed task file is missing: tasks/${file}`);
    }
  }
  if (Object.hasOwn(tasks.themes || {}, "copy_patch_jit")) {
    if (!goal.includes("bounded, explicitly gated exception")) {
      fail("copy_patch_jit theme requires GOAL.md to document it as a bounded, gated exception");
    }
    const copyPatchIds = new Set(
      tasks.items.filter((item) => item.theme === "copy_patch_jit").map((item) => item.id)
    );
    const gateIds = ["011", "016", "019", "026"];
    for (const id of copyPatchIds) {
      const item = tasks.items.find((candidate) => candidate.id === id);
      const dependsOnGate = (item.depends_on || []).some(
        (dependency) => gateIds.includes(dependency) || copyPatchIds.has(dependency)
      );
      if (!dependsOnGate) {
        fail(`copy_patch_jit task ${id} must depend on a correctness/dispatch gate (011, 016, 019, 026) or another copy_patch_jit task`);
      }
    }
  }
}
walk(runtimeRoot);

if (failures.length) {
  console.error(failures.map((failure) => `architecture: ${failure}`).join("\n"));
  process.exit(1);
}
console.log(JSON.stringify({
  schema: 1,
  status: "pass",
  checks: 8,
  catalog_rows: catalogRows.length,
  cold_fallbacks: coldFallbacks.length,
}));
