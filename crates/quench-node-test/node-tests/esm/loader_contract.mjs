// ESM loader contract: Node's error codes, constructors and messages for imports that
// cannot resolve or load, import attribute checks, CommonJS named exports found through
// TypeScript and Babel re-export helpers, regex literals with modifier-like classes, and
// the public shape of EventEmitter.
import { fileURLToPath } from 'node:url';
import EventEmitter from 'node:events';

const here = fileURLToPath(new URL('.', import.meta.url));
const scrub = (text) => text.split(here).join('<dir>/');
async function observe(label, specifier, options) {
  try {
    const namespace = await import(specifier, options);
    console.log(label, 'ok', JSON.stringify(namespace.default), Object.keys(namespace).sort().join());
  } catch (error) {
    console.log(label, error.constructor.name, error.code, scrub(error.message));
  }
}

await observe('unexported subpath', 'quench-exports-fixture/missing');
await observe('missing package', 'quench-missing-package-fixture');
await observe('missing scoped package', '@quench-missing/fixture/sub');
await observe('missing file', './missing.mjs');
await observe('json without attribute', './data.json');
await observe('json with attribute', './data.json', { with: { type: 'json' } });
await observe('javascript as json', './static_json.mjs', { with: { type: 'json' } });
await observe('unknown extension', './style.css');
await observe('unknown builtin', 'node:quench-missing');
await observe('unsupported scheme', 'https://example.invalid/module.js');
await observe('static json without attribute', './static_json.mjs');
await observe('static unexported subpath', './static_unexported.mjs');
await observe('typescript __exportStar', './cjs/typescript.cjs');
await observe('babel Object.keys re-export', './cjs/babel.cjs');

console.log('regex class with modifier text', /^[(?-i)y]$/i.test('Y'), /[a(?-m)]/.test('?'));

const shape = (object) => Reflect.ownKeys(object).map((key) => {
  const descriptor = Object.getOwnPropertyDescriptor(object, key);
  const value = descriptor.value;
  const kind = typeof value === 'function'
    ? `${JSON.stringify(value.name)}/${value.length}/${Object.getPrototypeOf(value).constructor.name}`
    : 'get' in descriptor ? 'accessor' : typeof value;
  return `${String(key)}:${kind}`;
}).sort().join(' ');
console.log('EventEmitter', shape(EventEmitter));
