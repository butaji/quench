// Layout training workload (tools/bolt-layout.sh): the npm package under QUENCH_LAYOUT_NODE_MODULES.
const { Linter } = require(process.env.QUENCH_LAYOUT_NODE_MODULES + '/eslint');
const linter = new Linter();
const msgs = linter.verify('var a = 1; if (a == 2) { console.log(a) }', { rules: { eqeqeq: 'error', 'no-unused-vars': 'error' } });
console.log(msgs.length);
