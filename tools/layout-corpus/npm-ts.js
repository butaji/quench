// Layout training workload (tools/bolt-layout.sh): the npm package under QUENCH_LAYOUT_NODE_MODULES.
const ts = require(process.env.QUENCH_LAYOUT_NODE_MODULES + '/typescript');
const out = ts.transpileModule('const x: number = 1; export function f<T>(a: T): T { return a; }', { compilerOptions: { module: ts.ModuleKind.CommonJS } });
console.log(out.outputText.length);
