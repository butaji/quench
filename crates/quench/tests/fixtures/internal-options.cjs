'use strict';
const options = require('internal/options');
const output = {};
output.exports = Object.keys(options).map(key => {
  const fn = options[key];
  const descriptor = Object.getOwnPropertyDescriptor(options, key);
  return [key, fn.name, fn.length, Object.hasOwn(fn, 'prototype'), descriptor.writable, descriptor.enumerable, descriptor.configurable];
});
output.module = [options === require('internal/options'), options === require('node:internal/options')];
const info = options.getCLIOptionsInfo();
const embedder = options.getEmbedderOptions();
const flags = options.getOptionsAsFlagsFromBinding();
output.identity = [info === options.getCLIOptionsInfo(), embedder === options.getEmbedderOptions(), flags === options.getOptionsAsFlagsFromBinding()];
output.embedder = [Object.getPrototypeOf(embedder) === null, embedder];
output.values = ['--warnings', '--pending-deprecation', '--conditions', '--require', '--report-dir', '--report-filename', '--report-on-signal', '--report-compact', '--test-concurrency', '--test-random-seed'].map(key => [key, options.getOptionValue(key)]);
const names = ['--warnings', '--pending-deprecation', '--conditions', '--require', '--report-dir', '--report-compact', '--test-random-seed'];
output.flags = flags.filter(flag => names.some(name => flag === name || flag.startsWith(name + '=') || flag === '--no-' + name.slice(2))).sort();
output.info = names.map(name => [name, info.options.get(name).type, info.options.get(name).defaultIsTrue]);
const conditions = options.getOptionValue('--conditions');
conditions.push('private-marker');
output.valueIdentity = options.getOptionValue('--conditions') === conditions;
options.refreshOptions();
output.refresh = [conditions !== options.getOptionValue('--conditions'), options.getOptionValue('--conditions').includes('private-marker'), info === options.getCLIOptionsInfo(), embedder === options.getEmbedderOptions(), flags === options.getOptionsAsFlagsFromBinding()];
let coerced = 0;
output.key = options.getOptionValue({ [Symbol.toPrimitive](hint) { coerced++; return '--warnings'; } });
output.coerced = coerced;
output.symbol = options.getOptionValue(Symbol('missing')) === undefined;
const sentinel = {};
try { options.getOptionValue({ [Symbol.toPrimitive]() { throw sentinel; } }); } catch (error) { output.keyThrow = error === sentinel; }
output.parse = [undefined, '--conditions="a b" --no-warnings', { toString() { return '--conditions=x'; } }].map(value => options.parseNodeOptionsEnvVar(value));
output.parseErrors = ['"unfinished', 'x\\'].map(value => { try { options.parseNodeOptionsEnvVar(value); } catch (error) { return [error.name, error.message]; } });
const processObject = process;
const originalEnv = process.env;
const originalWarning = process.emitWarning;
const warnings = [];
process.env = { NODE_TLS_REJECT_UNAUTHORIZED: 0 };
process.emitWarning = value => { warnings.push(value); throw sentinel; };
output.warning = [options.getAllowUnauthorized()];
process.env.NODE_TLS_REJECT_UNAUTHORIZED = '0';
try { options.getAllowUnauthorized(); } catch (error) { output.warning.push(error === sentinel); }
globalThis.process = { env: {} };
output.warning.push(options.getAllowUnauthorized());
globalThis.process = processObject;
options.refreshOptions();
output.warning.push(options.getAllowUnauthorized());
process.env = originalEnv;
process.emitWarning = originalWarning;
output.warning.push(warnings);
const schema = options.generateConfigJsonSchema();
const nextSchema = options.generateConfigJsonSchema();
output.schemaIdentity = [schema !== nextSchema, schema.$defs !== nextSchema.$defs, schema.$defs.config.properties.nodeOptions.properties !== nextSchema.$defs.config.properties.nodeOptions.properties];
function describe(value) {
  if (value === null || typeof value !== 'object') return value;
  if (Array.isArray(value)) return ['array', value.map(describe)];
  return [Object.getPrototypeOf(value) === null ? 'null' : 'object', Object.keys(value).map(key => [key, describe(value[key])])];
}
// CA default text reflects the engine's existing trust-store policy.
for (const name of ['use-openssl-ca', 'use-bundled-ca']) {
  const detail = schema.$defs.config.properties.nodeOptions.properties[name];
  detail.description = detail.description.replace(' (default)', '');
}
output.schema = describe(schema);
console.log(JSON.stringify(output));
