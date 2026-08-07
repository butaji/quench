#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
node - "$root/tools/focused-compat-policy.json" "$root" "${1:-}" <<'NODE'
const fs = require("fs");
const path = require("path");
const policy = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
const root = path.resolve(process.argv[3]);
const failureFile = process.argv[4];
if (!Array.isArray(policy.conflicts)) throw new Error("policy.conflicts must be an array");
const focusedStages = new Set(
  fs.readdirSync(path.join(root, "tests/node-compat"), { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && /^stage-\d+$/.test(entry.name))
    .map((entry) => Number(entry.name.slice(6))),
);
const policyStages = policy.conflicts.flatMap((entry) => entry.stages || []);
if (policyStages.some((stage) => !Number.isInteger(stage) || stage < 0)) {
  throw new Error("policy conflict stages must be non-negative integers");
}
const unknownPolicyStages = [...new Set(policyStages)].filter((stage) => !focusedStages.has(stage));
if (unknownPolicyStages.length) {
  throw new Error(`policy references missing focused stages: ${unknownPolicyStages.join(", ")}`);
}
const expected = new Set(
  failureFile && fs.existsSync(failureFile)
    ? fs.readFileSync(failureFile, "utf8").trim().split(/\s+/).filter(Boolean).map(Number)
    : []
);
const covered = new Set(policy.conflicts.flatMap((entry) => entry.stages));
const missing = [...expected].filter((stage) => !covered.has(stage));
if (missing.length) throw new Error(`unclassified focused conflicts: ${missing.join(", ")}`);
console.log(`policy_conflicts=${policy.conflicts.length}`);
console.log(`failure_list=${failureFile || "not supplied"}`);
console.log(`covered_failure_count=${expected.size}`);
NODE
