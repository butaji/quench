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
if (Module.findPackageJSON.name !== 'findPackageJSON' || Module.findPackageJSON.length !== 1) {
  throw new Error('findPackageJSON function shape');
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
const fixtureManifest = require.resolve('quench-fixture/package.json');
if (Module.findPackageJSON('quench-fixture', __filename) !== fixtureManifest) {
  throw new Error('findPackageJSON did not resolve a bare package');
}
if (Module.findPackageJSON('./node_modules/quench-fixture/index.js', __filename) !== fixtureManifest) {
  throw new Error('findPackageJSON did not resolve a relative module');
}
const fixtureUrl = new URL('file://' + require.resolve('quench-fixture/index.js'));
if (Module.findPackageJSON(fixtureUrl) !== fixtureManifest) {
  throw new Error('findPackageJSON did not accept a URL specifier');
}
if (Module.findPackageJSON('./node_modules/quench-fixture/index.js', new URL('file://' + __filename)) !== fixtureManifest) {
  throw new Error('findPackageJSON did not accept a URL base');
}
const path = require('node:path');
const repoRoot = path.resolve(__dirname, '../../..');
if (Module.findPackageJSON('./package.json', repoRoot + path.sep) !== require.resolve('../../../package.json')) {
  throw new Error('findPackageJSON did not resolve a directory base');
}
let dirnameBaseError;
try { Module.findPackageJSON('./package.json', repoRoot); } catch (caught) { dirnameBaseError = caught; }
if (!dirnameBaseError || dirnameBaseError.code !== 'ERR_MODULE_NOT_FOUND') {
  throw new Error('findPackageJSON treated a path base as a directory without a trailing separator');
}
const urlModule = require('node:url');
if (urlModule.URL !== URL || urlModule.pathToFileURL.name !== 'pathToFileURL' ||
  urlModule.pathToFileURL.length !== 2 || urlModule.fileURLToPath.name !== 'fileURLToPath' ||
  urlModule.fileURLToPath.length !== 1) {
  throw new Error('node:url function shape or URL identity');
}
const specialPath = path.resolve('a b#c%.js');
const specialUrl = urlModule.pathToFileURL(specialPath);
if (!(specialUrl instanceof URL) || !specialUrl.href.endsWith('/a%20b%23c%25.js') ||
  urlModule.fileURLToPath(specialUrl) !== specialPath ||
  urlModule.fileURLToPath(specialUrl.href) !== specialPath) {
  throw new Error('node:url file path conversion');
}
for (const [input, code] of [
  ['https://example.com/file', 'ERR_INVALID_URL_SCHEME'],
  ['file://example.com/file', 'ERR_INVALID_FILE_URL_HOST'],
  ['file:///tmp/a%2fb', 'ERR_INVALID_FILE_URL_PATH'],
]) {
  let error;
  try { urlModule.fileURLToPath(input); } catch (caught) { error = caught; }
  if (!(error instanceof TypeError) || error.code !== code) {
    throw new Error('fileURLToPath error for ' + input + ': ' + (error && error.code));
  }
}
if (urlModule.pathToFileURL('C:\\foo bar\\baz.js', { windows: true }).href !==
  'file:///C:/foo%20bar/baz.js' ||
  urlModule.fileURLToPath('file:///C:/foo%20bar', { windows: true }) !== 'C:\\foo bar' ||
  urlModule.fileURLToPath('file://server/share/a', { windows: true }) !== '\\\\server\\share\\a') {
  throw new Error('node:url Windows path conversion option');
}
for (const [specifier, base, code, type] of [
  ['node:fs', __filename, 'ERR_INVALID_URL_SCHEME', TypeError],
  ['missing-quench-package', __filename, 'ERR_MODULE_NOT_FOUND', Error],
  ['./missing-quench-module.js', __filename, 'ERR_MODULE_NOT_FOUND', Error],
  ['./relative.js', undefined, 'ERR_UNSUPPORTED_RESOLVE_REQUEST', TypeError],
  ['./relative.js', 'https://example.com/entry.js', 'ERR_INVALID_URL_SCHEME', TypeError],
  ['file:///tmp/quench-module-that-does-not-exist.js', undefined, 'ERR_MODULE_NOT_FOUND', Error],
]) {
  let error;
  try { Module.findPackageJSON(specifier, base); } catch (caught) { error = caught; }
  if (!(error instanceof type) || error.code !== code) {
    throw new Error('findPackageJSON error for ' + specifier + ': ' + (error && error.code));
  }
}
for (const filename of [undefined, 'relative/file.js', null]) {
  let error;
  try { Module.createRequire(filename); } catch (caught) { error = caught; }
  if (!(error instanceof TypeError) || error.code !== 'ERR_INVALID_ARG_VALUE') {
    throw new Error('createRequire validation for ' + String(filename));
  }
}
console.log('modules: %d builtins, %d v1 modules', Module.builtinModules.length, expected.length);
