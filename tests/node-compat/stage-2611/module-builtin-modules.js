const assert = require('assert');
const moduleApi = require('module');

assert(Array.isArray(moduleApi.builtinModules));
assert(moduleApi.builtinModules.includes('fs'));
assert(moduleApi.builtinModules.includes('module'));
console.log('module builtinModules: ok');
