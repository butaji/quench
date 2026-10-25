// Layout training workload (tools/bolt-layout.sh): the npm package under QUENCH_LAYOUT_NODE_MODULES.
const esprima = require(process.env.QUENCH_LAYOUT_NODE_MODULES + '/esprima'); const fs = require('fs');
const src = fs.readFileSync(process.env.QUENCH_LAYOUT_NODE_MODULES + '/semver/classes/range.js', 'utf8') + fs.readFileSync(process.env.QUENCH_LAYOUT_NODE_MODULES + '/diff/lib/index.js', 'utf8').slice(0, 60000);
const toks = esprima.tokenize(src, { range: true }); const ast = esprima.parseScript(fs.readFileSync(process.env.QUENCH_LAYOUT_NODE_MODULES + '/semver/classes/range.js', 'utf8'));
console.log(toks.length, ast.body.length);
