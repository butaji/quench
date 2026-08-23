#!/usr/bin/env node
"use strict";
const assert = require("assert");
const fs = require("fs");
const path = require("path");

const schemaPath = path.join(__dirname, "perf-workload.schema.json");
const schema = JSON.parse(fs.readFileSync(schemaPath, "utf8"));
const fields = ["iterations", "checksum", "allocations", "copies", "bytes", "peak_rss", "wall_ms"];

assert.strictEqual(schema.type, "object");
assert.strictEqual(schema.additionalProperties, false);
assert.deepStrictEqual(schema.required, fields);
assert.deepStrictEqual(Object.keys(schema.properties).sort(), [...fields].sort());
for (const field of fields) {
  assert.ok(schema.properties[field], `${field} must have a schema`);
  assert.ok(!schema.properties[field].nullable, `${field} must not be nullable`);
}
for (const field of fields.slice(0, -1)) {
  assert.strictEqual(schema.properties[field].type, "integer");
  assert.strictEqual(schema.properties[field].minimum, field === "peak_rss" ? 1 : 0);
}
assert.strictEqual(schema.properties.wall_ms.type, "number");
assert.strictEqual(schema.properties.wall_ms.minimum, 0);

// Keep the executable producer and declarative contract in lockstep without
// running a workload: every emitted source key must be declared exactly once.
const source = fs.readFileSync(path.join(__dirname, "perf-workload.cjs"), "utf8");
const emitted = source.match(/JSON\.stringify\(\{([^}]+)\}\)/)?.[1] ?? "";
const sourceFields = [...emitted.matchAll(/(?:^|,)\s*(?:([A-Za-z_]+)\s*:)?([A-Za-z_]+)/g)].map((m) => m[1] || m[2]);
assert.deepStrictEqual(sourceFields, fields);
console.log("perf-workload-schema: source and JSON schema contract verified");
