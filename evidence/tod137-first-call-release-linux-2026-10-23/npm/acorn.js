const acorn = require('/opt/node-tools/node_modules/acorn');
const src = require('fs').readFileSync(require.resolve('/opt/node-tools/node_modules/acorn'), 'utf8');
console.log(acorn.parse(src, { ecmaVersion: 'latest', sourceType: 'module' }).body.length);
