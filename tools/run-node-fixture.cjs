const Module = require("module");
const path = require("path");
const fs = require("fs");
const cp = require("child_process");

const file = path.resolve(process.argv[2]);
if (!file) throw new Error("fixture path is required");

// Node's test runner executes from the Node checkout root. Reproduce that
// setup for fixtures invoked from quench-node's repository root so tests that
// use paths such as ./test/fixtures resolve correctly.
const nodeRoot = path.resolve(path.dirname(file), "../..");
if (
  path.basename(nodeRoot) === "node" &&
  path.basename(path.dirname(nodeRoot)) === "tests"
) {
  process.chdir(nodeRoot);
}

// _compile() is for CommonJS fixtures. It is not an ESM runner and can return
// without executing an .mjs entry, so delegate those fixtures to real Node.
if (file.endsWith(".mjs")) {
  const result = cp.spawnSync(process.execPath, [file], {
    cwd: process.cwd(),
    stdio: "inherit",
  });
  if (result.error) throw result.error;
  process.exit(result.status === null ? 1 : result.status);
}

const fixture = new Module(file, module);
fixture.filename = file;
fixture.paths = Module._nodeModulePaths(path.dirname(file));
const source = fs.readFileSync(file, "utf8");
const flags = !process.env.QUENCH_NODE_FLAGS_CHILD &&
  source.match(/^\s*\/\/\s*Flags:\s*(.+)$/m)?.[1]
    ?.trim()
    .split(/\s+/)
    .filter(Boolean);
if (flags?.length) {
  const childArgs = file.endsWith(".js")
    ? [...flags, __filename, file]
    : [...flags, file];
  const result = cp.spawnSync(process.execPath, childArgs, {
    cwd: process.cwd(),
    stdio: "inherit",
    env: { ...process.env, QUENCH_NODE_FLAGS_CHILD: "1" },
  });
  if (result.error) throw result.error;
  process.exit(result.status === null ? 1 : result.status);
}
fixture._compile(source, file);
