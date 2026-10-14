// Guest fixture: descriptors identify callable API obligations, never behavior coverage.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

const name = process.argv[2];
const inventory = JSON.parse(fs.readFileSync(fileURLToPath(new URL('inventory.json', import.meta.url)), 'utf8'));
const pinned = inventory.packages.find(p => p.name === name);
const installation = pinned.installation ?? name;
const directory = path.join(path.dirname(fileURLToPath(import.meta.url)), 'node_modules', installation);
const metadata = JSON.parse(fs.readFileSync(path.join(directory, 'package.json'), 'utf8'));
const functions = [];
const aliases = [];
const limitations = ['Returned objects/functions and dynamically installed methods need behavioral fixtures.',
  'Descriptors do not establish behavior coverage.', 'Type-only subpath exports need declaration tests.'];
const failures = [];
const cliEntrypoints = [];
const seen = new Map();
const intrinsic = new Set([Object.prototype, Function.prototype, Array.prototype,
  String.prototype, Number.prototype, Boolean.prototype, RegExp.prototype,
  Date.prototype, Map.prototype, Set.prototype, Promise.prototype, Error.prototype]);
const keyName = key => typeof key === 'symbol' ? `[${String(key)}]` : JSON.stringify(key);

function walk(value, location) {
  if (value === null || !['function', 'object'].includes(typeof value) || intrinsic.has(value)) return;
  if (typeof value === 'function') {
    functions.push({ path: location, kind: 'call', name: value.name, length: value.length });
  }
  if (seen.has(value)) { aliases.push({ path: location, target: seen.get(value) }); return; }
  seen.set(value, location);
  for (const key of Reflect.ownKeys(value).sort((a, b) => keyName(a) < keyName(b) ? -1 : keyName(a) > keyName(b) ? 1 : 0)) {
    if (typeof value === 'function' && ['caller', 'arguments', 'name', 'length'].includes(key)) continue;
    const descriptor = Object.getOwnPropertyDescriptor(value, key);
    const child = `${location}[${keyName(key)}]`;
    for (const kind of ['get', 'set']) if (typeof descriptor[kind] === 'function') {
      const fn = descriptor[kind];
      functions.push({ path: child, kind, name: fn.name, length: fn.length });
      if (kind === 'get') limitations.push(`Getter result not traversed: ${child}`);
    }
    if ('value' in descriptor) walk(descriptor.value, child);
  }
  const prototype = Object.getPrototypeOf(value);
  if (prototype && !intrinsic.has(prototype)) walk(prototype, `${location}.[[Prototype]]`);
}

const exports = metadata.exports;
const subpaths = exports && typeof exports === 'object' && !Array.isArray(exports)
  && Object.keys(exports).some(k => k.startsWith('.')) ? Object.keys(exports).filter(k => !k.includes('*')) : ['.'];
function targets(value) {
  if (typeof value === 'string') return [value];
  if (!value || typeof value !== 'object') return [];
  return Object.entries(value).filter(([key]) => key !== 'types').flatMap(([, child]) => targets(child));
}
function files(directory, prefix = './') {
  const result = [];
  for (const item of fs.readdirSync(directory, { withFileTypes: true })) {
    if (item.name === 'node_modules') continue;
    const location = prefix + item.name;
    if (item.isDirectory()) result.push(...files(path.join(directory, item.name), location + '/'));
    else if (item.isFile()) result.push(location);
  }
  return result;
}
if (exports && typeof exports === 'object') for (const key of Object.keys(exports)) {
  if (!key.includes('*')) continue;
  let matched = false;
  for (const target of targets(exports[key])) {
    if (!target.includes('*') || target.endsWith('.d.ts') || !target.startsWith('./')) continue;
    const [prefix, suffix] = target.split('*');
    for (const file of files(directory)) {
      if (file.startsWith(prefix) && file.endsWith(suffix)) {
        const capture = file.slice(prefix.length, suffix.length ? -suffix.length : undefined);
        subpaths.push(key.replaceAll('*', capture));
        matched = true;
      }
    }
  }
  if (!matched) limitations.push(`Unexpanded export pattern: ${key}`);
}
if (!pinned.types_only && !pinned.native_only) {
  for (const subpath of [...new Set(subpaths)].sort()) {
    if (subpath.endsWith('.json') || subpath === './package.json') continue;
    const specifier = name + (subpath === '.' ? '' : subpath.slice(1));
    const bins = typeof metadata.bin === 'string' ? [metadata.bin] : Object.values(metadata.bin ?? {});
    if (exports && typeof exports === 'object' && targets(exports[subpath]).some(target =>
      bins.some(bin => './' + bin.replace(/^\.\//, '') === target))) {
      cliEntrypoints.push(specifier);
      continue;
    }
    try {
      const actual = specifier === 'punycode' ? 'punycode/punycode.js'
        : specifier === 'string_decoder' ? 'string_decoder/lib/string_decoder.js'
        : specifier === 'buffer' ? 'buffer/index.js' : installation + specifier.slice(name.length);
      const namespace = await import(actual);
      walk(namespace, `import(${JSON.stringify(specifier)})`);
    } catch (error) {
      failures.push({ specifier, code: error.code ?? null, name: error.name, message: error.message });
    }
    // CJS and ESM condition branches can expose different APIs.
    try {
      const actual = specifier === 'punycode' ? 'punycode/punycode.js'
        : specifier === 'string_decoder' ? 'string_decoder/lib/string_decoder.js'
        : specifier === 'buffer' ? 'buffer/index.js' : installation + specifier.slice(name.length);
      walk(createRequire(import.meta.url)(actual), `require(${JSON.stringify(specifier)})`);
    } catch (error) {
      failures.push({ specifier: `require:${specifier}`, code: error.code ?? null, name: error.name, message: error.message });
    }
  }
}
functions.sort((a, b) => a.path + a.kind < b.path + b.kind ? -1 : a.path + a.kind > b.path + b.kind ? 1 : 0);
aliases.sort((a, b) => a.path < b.path ? -1 : a.path > b.path ? 1 : 0);
console.log(JSON.stringify({ package: name, version: metadata.version,
  kind: pinned.types_only ? 'declarations' : pinned.native_only ? 'native' : 'javascript',
  functions, aliases, failures, cliEntrypoints, limitations: limitations.sort(),
  behaviorCoverage: 'unqualified' }));
