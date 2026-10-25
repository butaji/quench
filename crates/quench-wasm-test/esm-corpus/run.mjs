// Imports each spec module of a Wasm-ESM training corpus (`wasm-esm-corpus`) through Node's
// Wasm ESM integration and invokes its actions. `returns` skips the actions expected to trap,
// for profilers that cannot follow the JIT's trap signals.
import { readFileSync } from "node:fs";

const returnsOnly = process.argv[2] === "returns";
const modules = JSON.parse(readFileSync(new URL("./corpus.json", import.meta.url), "utf8"));
const view = new DataView(new ArrayBuffer(BigUint64Array.BYTES_PER_ELEMENT));
const value = ([type, bits]) => {
  switch (type) {
    case "i32": return Number(BigInt.asIntN(32, BigInt(bits)));
    case "i64": return BigInt.asIntN(64, BigInt(bits));
    case "f32": view.setUint32(0, Number(bits)); return view.getFloat32(0);
    default: view.setBigUint64(0, BigInt(bits)); return view.getFloat64(0);
  }
};
let calls = 0, traps = 0;
for (const { file, actions } of modules) {
  let namespace;
  try { namespace = await import(`./${file}`); } catch { continue; }
  for (const [field, args, returns] of actions) {
    if (returnsOnly && !returns) continue;
    try { namespace[field](...args.map(value)); calls++; } catch { traps++; }
  }
}
console.log(`modules ${modules.length} calls ${calls} traps ${traps}`);
