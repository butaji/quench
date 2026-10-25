// Layout training workload (tools/bolt-layout.sh): the npm package under QUENCH_LAYOUT_NODE_MODULES.
const acorn = require(process.env.QUENCH_LAYOUT_NODE_MODULES + '/acorn');
const src = require('fs').readFileSync(require.resolve(process.env.QUENCH_LAYOUT_NODE_MODULES + '/acorn'), 'utf8');
console.log(acorn.parse(src, { ecmaVersion: 'latest', sourceType: 'module' }).body.length);
