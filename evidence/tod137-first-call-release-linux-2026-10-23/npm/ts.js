const ts = require('/opt/node-tools/node_modules/typescript');
const out = ts.transpileModule('const x: number = 1; export function f<T>(a: T): T { return a; }', { compilerOptions: { module: ts.ModuleKind.CommonJS } });
console.log(out.outputText.length);
