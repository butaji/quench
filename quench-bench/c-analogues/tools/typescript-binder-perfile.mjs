// Per-file binder reference for the typescript-lib analogue (Stage 2).
//
// For each source file of immer-tiny (files.json), parses it with ts.createSourceFile and binds it
// with ts.bindSourceFile under the project's compiler options, then prints:
//   fileName, symbolCount (the binder's own counter, file.symbolCount), flowNodeCount
// flowNodeCount is not exposed by TypeScript, so a copy of lib/typescript.js is written to the
// output directory with createFlowNode instrumented to count calls; the counter is reset per file.
//
// usage: node typescript-binder-perfile.mjs <jetstream-root> <ts-package-dir> <scratch-dir>
import { createRequire } from "node:module";
import fs from "node:fs";
import path from "node:path";

const [root, pkg, scratch] = process.argv.slice(2);
const src = fs.readFileSync(path.join(pkg, "lib/typescript.js"), "utf8");
const marker = "function createFlowNode(flags, node, antecedent) {\n";
if (src.split(marker).length !== 2) throw new Error("createFlowNode marker not unique");
const instrumented = src.replace(marker, marker + "  globalThis.__flowNodes = (globalThis.__flowNodes || 0) + 1;\n");
const dir = path.join(scratch, "instr");
fs.mkdirSync(dir, { recursive: true });
fs.writeFileSync(path.join(dir, "typescript.js"), instrumented);

const require = createRequire(import.meta.url);
const ts = require(path.join(dir, "typescript.js"));
const tsConfig = JSON.parse(fs.readFileSync(path.join(root, "TypeScript/src/gen/immer-tiny/actually-a-type-script-config-please-dont-firewall-me.json"), "utf8"));
const files = JSON.parse(fs.readFileSync(path.join(root, "TypeScript/src/gen/immer-tiny/files.json"), "utf8"));
const options = ts.convertCompilerOptionsFromJson(tsConfig.compilerOptions, "/User/test/JetStream/").options;
options.lib = [...(options.lib || []), "dom"];

for (const name of Object.keys(files)) {
  const sf = ts.createSourceFile(name, files[name], 99);
  globalThis.__flowNodes = 0;
  ts.bindSourceFile(sf, options);
  console.log([name, sf.symbolCount ?? 0, globalThis.__flowNodes].join("\t"));
}
