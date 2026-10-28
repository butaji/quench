// Fixed-work JetStream Wasm fixture: jetstream-fixed.cjs <glue.js> <module.wasm> <benchmark.js> <iterations>
const fs = require('fs'), vm = require('vm');
const [glue, wasm, bench, n] = process.argv.slice(2);
globalThis.require = require; globalThis.__dirname = __dirname;
globalThis.Module = { wasmBinary: fs.readFileSync(wasm), print() {}, printErr() {} };
vm.runInThisContext(fs.readFileSync(glue, 'utf8') + `\n;globalThis.setupModule = setupModule;`);
vm.runInThisContext(fs.readFileSync(bench, 'utf8') + `\n;globalThis.Benchmark = Benchmark;`);
(async () => {
  const b = new Benchmark();
  for (let i = 0; i < Number(n); i++) await b.runIteration();
  if (b.validate) b.validate();
  console.log('ok');
})();
