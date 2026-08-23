const assert = require('assert');
const moduleApi = require('module');

for (const name of ['dgram', 'constants', 'domain', 'util/types']) {
  assert(moduleApi.isBuiltin(name), `${name} should be builtin`);
  assert(moduleApi.isBuiltin(`node:${name}`), `node:${name} should be builtin`);
  assert(moduleApi.builtinModules.includes(name), `${name} should be listed`);
}
assert(!moduleApi.isBuiltin('not-a-node-module'));
console.log('module builtin inventory: ok');
