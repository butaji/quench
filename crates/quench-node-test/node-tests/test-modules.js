// Node compat: require('node:mod') for every v1 module.
const expected = ['assert','buffer','console','dns','events','fs','net',
  'module','os','path','process','querystring','stream','timers','tty','url','util'];
for (const name of expected) {
  const m = require('node:' + name);
  if (typeof m !== 'object' && typeof m !== 'function') {
    throw new Error(name + ': bad type ' + typeof m);
  }
}
const Module = require('node:module');
if (Module !== require('module')) throw new Error('builtin module identity');
if (!Array.isArray(Module.builtinModules)) throw new Error('builtinModules: bad type');
if (Module.isBuiltin.name !== 'isBuiltin' || Module.isBuiltin.length !== 1) {
  throw new Error('isBuiltin function shape');
}
if (Module.createRequire.name !== 'createRequire' || Module.createRequire.length !== 1) {
  throw new Error('createRequire function shape');
}
for (const name of ['fs', 'fs/promises', 'path/posix', 'assert/strict', 'module']) {
  if (!Module.builtinModules.includes(name)) throw new Error('missing builtin: ' + name);
  if (!Module.isBuiltin(name)) throw new Error('isBuiltin(' + name + ')');
  if (!Module.isBuiltin('node:' + name)) throw new Error('isBuiltin(node:' + name + ')');
}
for (const name of ['not-a-builtin', 'node:fs/nope', 'fs/nope', null, 0, {}]) {
  if (Module.isBuiltin(name)) throw new Error('unexpected builtin: ' + String(name));
}
if (Module.builtinModules.some(name =>
  name.startsWith('node:') && !['node:sea', 'node:sqlite', 'node:test', 'node:test/reporters'].includes(name))) {
  throw new Error('builtinModules contains an unexpected node: entry');
}
const packageRequire = Module.createRequire(require.resolve('./test-modules.js'));
const add = packageRequire('quench-fixture');
if (typeof add !== 'function' || add(2, 3) !== 5 || add.depLabel !== 'quench-dep') {
  throw new Error('createRequire did not resolve the fixture package');
}
for (const filename of [undefined, 'relative/file.js', null]) {
  let error;
  try { Module.createRequire(filename); } catch (caught) { error = caught; }
  if (!(error instanceof TypeError) || error.code !== 'ERR_INVALID_ARG_VALUE') {
    throw new Error('createRequire validation for ' + String(filename));
  }
}
console.log('modules: %d builtins, %d v1 modules', Module.builtinModules.length, expected.length);
