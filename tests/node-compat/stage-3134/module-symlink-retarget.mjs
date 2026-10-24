// One real-path cache serves both module loaders: after `node_modules/pkg` is retargeted from
// package `a` to package `b`, require and import keep resolving through the real path the first
// load recorded, in either order. Each order runs in a fresh process so the caches start empty.
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const RUNNER = `
import fs from 'node:fs';
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const link = new URL('./node_modules/pkg', import.meta.url).pathname;
const order = process.argv[2];
const retarget = (to) => { fs.rmSync(link, { force: true }); fs.symlinkSync(to, link); };
const out = [];
retarget('../../a');
if (order === 'cjs-first') {
  out.push(require('pkg'));
  retarget('../../b');
  out.push(require('pkg/other.js'), require.resolve('pkg/other.js').slice(ROOT.length + 1));
  out.push((await import('pkg/esm.mjs')).default);
} else {
  out.push((await import('pkg/esm.mjs')).default);
  retarget('../../b');
  out.push((await import('pkg/index.js')).default, require('pkg/other.js'));
}
console.log(order, out.join(' '));
`;

// Node 26's output for each order.
const EXPECTED = {
  "cjs-first": "cjs-first a-index a-other a/other.js a-esm",
  "esm-first": "esm-first a-esm a-index a-other",
};

const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "module-symlink-retarget-")));
try {
  for (const name of ["a", "b"]) {
    const dir = path.join(root, name);
    fs.mkdirSync(dir);
    fs.writeFileSync(path.join(dir, "package.json"), '{"name":"pkg"}');
    fs.writeFileSync(path.join(dir, "index.js"), `module.exports = '${name}-index'`);
    fs.writeFileSync(path.join(dir, "other.js"), `module.exports = '${name}-other'`);
    fs.writeFileSync(path.join(dir, "esm.mjs"), `export default '${name}-esm';`);
  }
  fs.mkdirSync(path.join(root, "app/node_modules"), { recursive: true });
  const runner = path.join(root, "app/run.mjs");
  fs.writeFileSync(runner, `const ROOT = ${JSON.stringify(root)};\n${RUNNER}`);
  for (const [order, expected] of Object.entries(EXPECTED)) {
    const child = spawnSync(process.execPath, [runner, order], { encoding: "utf8" });
    const line = child.stdout.trim();
    if (child.status !== 0 || line !== expected) {
      throw new Error(`${order}: status ${child.status}, got ${JSON.stringify(line)}\n${child.stderr}`);
    }
    console.log(line);
  }
} finally {
  fs.rmSync(root, { recursive: true, force: true });
}
