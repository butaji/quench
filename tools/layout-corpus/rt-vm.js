// Layout training: package sources evaluated in fresh vm contexts, the way script hosts load code.
const fs = require('fs'), vm = require('vm'), path = require('path'), zlib = require('zlib');
const M = process.env.QUENCH_LAYOUT_NODE_MODULES;
const files = ['acorn/dist/acorn.js', 'esprima/dist/esprima.js', 'semver/functions/parse.js', 'diff/dist/diff.js'];
let n = 0;
for (const f of files) {
  const src = fs.readFileSync(path.join(M, f), 'utf8');
  const z = zlib.inflateSync(zlib.deflateSync(Buffer.from(src))).toString('utf8');
  const ctx = vm.createContext({ console, module: { exports: {} }, exports: {}, require, self: {} });
  try { vm.runInContext(z, ctx, { filename: f }); n += Object.keys(ctx.module.exports).length; } catch (e) { n += 1; }
}
const ctx2 = vm.createContext({});
for (let i = 0; i < 200; i++) n += vm.runInContext('(function(a){ return a.map(x => x * 2).length })([1,2,3])', ctx2);
const t0 = Date.now(); let s = 0; for (let i = 0; i < 300000; i++) s += Math.sqrt(i) | 0; console.log(n, s > 0, Date.now() >= t0);
