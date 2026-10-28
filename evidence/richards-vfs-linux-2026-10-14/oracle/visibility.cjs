'use strict';

const moduleApi = require('node:module');
let required;
try {
  const api = require('node:vfs');
  const instance = api.create();
  instance.writeFileSync('/probe', 'enabled');
  required = { text: instance.readFileSync('/probe', 'utf8') };
} catch (error) {
  required = { name: error.name, code: error.code };
}
let bare;
try {
  require('vfs');
  bare = { succeeded: true };
} catch (error) {
  bare = { name: error.name, code: error.code };
}
const builtin = process.getBuiltinModule('node:vfs');
console.log(JSON.stringify({
  listed: moduleApi.builtinModules.includes('node:vfs'),
  builtin: moduleApi.isBuiltin('node:vfs'),
  bareBuiltin: moduleApi.isBuiltin('vfs'),
  getBuiltin: builtin === undefined ? 'undefined' : typeof builtin.create,
  required,
  bare,
}));
