import { readFileSync } from "node:fs";
const bytes = readFileSync(process.argv[2]);
const steps = [1,1,1,1,2000]; let reads = 0, now = 0;
const clock_ms = () => { const value = now; now += steps[Math.min(reads, steps.length - 1)]; reads++; return value | 0; };
const { instance } = await WebAssembly.instantiate(bytes, { env: { clock_ms } });
console.log(` - Score: ${instance.exports.run()} reads ${reads}`);
