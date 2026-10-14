// CoreMark runner for JS engines with the wasm-coremark-rs host contract:
// import env.clock_ms: () -> i32 (ms since first read), export run: () -> f32.
import { readFileSync } from "node:fs";
const bytes = readFileSync(process.argv[2]);
let start;
const clock_ms = () => { const now = performance.now(); start ??= now; return (now - start) | 0; };
const { instance } = await WebAssembly.instantiate(bytes, { env: { clock_ms } });
console.log(` - Score: ${instance.exports.run()}`);
