const { Linter } = require('/opt/node-tools/node_modules/eslint');
const linter = new Linter();
const msgs = linter.verify('var a = 1; if (a == 2) { console.log(a) }', { rules: { eqeqeq: 'error', 'no-unused-vars': 'error' } });
console.log(msgs.length);
