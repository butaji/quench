#!/usr/bin/env node

const crypto = require("crypto");
const fs = require("fs");
const os = require("os");
const path = require("path");
const cp = require("child_process");

const root = path.resolve(process.argv[2] || path.join(__dirname, ".."));
const fixtureRoot = path.resolve(
  process.argv[3] || path.join(root, "tests/node/test/parallel"),
);

function filesUnder(target) {
  if (!fs.existsSync(target)) return [];
  const stat = fs.statSync(target);
  if (stat.isFile()) return [target];
  const files = [];
  for (const entry of fs.readdirSync(target, { withFileTypes: true })) {
    const full = path.join(target, entry.name);
    if (entry.isDirectory()) files.push(...filesUnder(full));
    else if (entry.isFile()) files.push(full);
  }
  return files.sort();
}

function digest(targets) {
  const hash = crypto.createHash("sha256");
  const files = targets.flatMap(filesUnder).sort();
  for (const file of files) {
    const relative = path.relative(root, file);
    hash.update(relative);
    hash.update("\0");
    hash.update(fs.readFileSync(file));
    hash.update("\0");
  }
  return { digest: hash.digest("hex"), files: files.length };
}

function command(commandName, args) {
  try {
    return cp.execFileSync(commandName, args, {
      cwd: root,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    }).trim();
  } catch {
    return null;
  }
}

const binary = process.env.QUENCH_NODE_BIN;
const binaryDigest = binary && fs.existsSync(binary)
  ? crypto.createHash("sha256").update(fs.readFileSync(binary)).digest("hex")
  : null;
const source = digest([
  path.join(root, "crates/quench-node"),
  path.join(root, "tools/run-node-fixture.cjs"),
]);
const fixtures = digest([fixtureRoot]);
const focused = digest([path.join(root, "tests/node-compat")]);
const ownership = digest([path.join(root, "tools/compat-ownership.json")]);
const status = command("git", ["status", "--porcelain=v1"]);

process.stdout.write(`${
  JSON.stringify({
    schema: 1,
    generated_at: new Date().toISOString(),
    root,
    fixture_root: path.relative(root, fixtureRoot) || ".",
    node_version: process.version,
    platform: `${process.platform}-${process.arch}`,
    git_commit: command("git", ["rev-parse", "HEAD"]),
    working_tree_dirty: Boolean(status),
    source_digest: source.digest,
    source_files: source.files,
    fixture_digest: fixtures.digest,
    fixture_files: fixtures.files,
    focused_digest: focused.digest,
    focused_files: focused.files,
    ownership_digest: ownership.digest,
    binary: binary ? path.resolve(binary) : null,
    binary_digest: binaryDigest,
  })
}\n`);
