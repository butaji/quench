// Test262 case execution on a Node-compatible host (Node, Bun): the pinned harness
// composition, the $262 host object and the per-mode verdict. Shared by the Node
// oracle and the cross-engine case runner.
'use strict';

const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { pathToFileURL, fileURLToPath } = require('node:url');

const REPOSITORY = path.resolve(__dirname, '../..');
const SUITE = process.env.TEST262_DIR
  ? path.resolve(process.env.TEST262_DIR)
  : path.join(REPOSITORY, 'tests/test262');
// The longest an async case may take to call $DONE.
const ASYNC_DONE_TIMEOUT_MS = 5000;
const DEFAULT_HARNESS = ['assert.js', 'sta.js'];
const ASYNC_HARNESS = 'doneprintHandle.js';
// The lines doneprintHandle.js's $DONE prints, which report an async case's completion.
const ASYNC_COMPLETE = 'Test262:AsyncTestComplete';
const ASYNC_FAILURE_PREFIX = 'Test262:AsyncTestFailure:';

function frontmatter(source) {
  const match = /\/\*---([\s\S]*?)---\*\//.exec(source);
  return match ? match[1] : '';
}

function yamlList(yaml, key) {
  const inline = new RegExp(`^\\s*${key}:\\s*\\[([^\\]]*)\\]`, 'm').exec(yaml);
  if (inline) return inline[1].split(',').map((item) => item.trim()).filter(Boolean);
  const block = new RegExp(`^\\s*${key}:\\s*\\n((?:\\s+-\\s+.*\\n?)+)`, 'm').exec(yaml);
  if (!block) return [];
  return block[1].split('\n').map((line) => line.replace(/^\s*-\s*/, '').trim()).filter(Boolean);
}

function metadata(source) {
  const yaml = frontmatter(source);
  const negative = yaml.indexOf('negative:');
  return {
    flags: yamlList(yaml, 'flags'),
    includes: yamlList(yaml, 'includes'),
    features: yamlList(yaml, 'features'),
    negativeType: negative < 0 ? null : (/type:\s*(\w+)/.exec(yaml.slice(negative)) || [])[1],
  };
}

// One real realm and module-record cache per case (and per $262.createRealm).
// DONT_CONTEXTIFY gives Script code an ordinary global object, including its
// specified extensibility behavior. Engines without it retain their VM contract.
function realm(onPrint, referrer) {
  const context = vm.createContext(vm.constants?.DONT_CONTEXTIFY);
  const global = vm.runInContext('this', context);
  const modules = new Map();
  let linking = Promise.resolve();
  let missingCapability = null;
  const unsupported = (message) => {
    missingCapability ??= message;
    throw new Error(message);
  };
  const resolve = (specifier, parent) => {
    const url = new URL(specifier, parent);
    if (url.protocol !== 'file:') unsupported(`oracle module protocol unavailable: ${url.protocol}`);
    return url.href;
  };
  const record = (identifier) => {
    if (modules.has(identifier)) return modules.get(identifier);
    if (typeof vm.SourceTextModule !== 'function') unsupported('oracle needs vm.SourceTextModule');
    const source = fs.readFileSync(fileURLToPath(identifier), 'utf8');
    const entry = { module: new vm.SourceTextModule(source, {
      context, identifier,
      initializeImportMeta(meta) { meta.url = identifier; },
      importModuleDynamically: dynamicImport,
    }), evaluation: null };
    modules.set(identifier, entry);
    return entry;
  };
  const link = (entry) => {
    // Linking is serialized, evaluation is not: a pending top-level await must
    // leave other imports able to link and start their own graphs.
    const result = linking.then(async () => {
      if (entry.module.status === 'unlinked') {
        await entry.module.link((specifier, parent, attributes) => {
          if (Object.keys(attributes?.attributes ?? attributes?.assert ?? {}).length) {
            unsupported('oracle typed module imports are unavailable');
          }
          return record(resolve(specifier, parent.identifier)).module;
        });
      }
    });
    linking = result.catch(() => {});
    return result;
  };
  const evaluate = async (entry) => {
    await link(entry);
    // Keep the evaluation promise, not status: Node can report "evaluated"
    // while a module's top-level await is still pending.
    entry.evaluation ??= entry.module.evaluate();
    await entry.evaluation;
    return entry.module;
  };
  async function dynamicImport(specifier, parent, attributes) {
    if (Object.keys(attributes ?? {}).length) unsupported('oracle typed module imports are unavailable');
    return evaluate(record(resolve(specifier, parent.identifier ?? referrer)));
  }
  const script = (source, filename = referrer) => vm.runInContext(source, context, {
    filename, importModuleDynamically: dynamicImport,
  });
  const $262 = {
    global,
    createRealm: () => realm(onPrint, referrer).host,
    evalScript: (source) => script(source),
    detachArrayBuffer: (buffer) => structuredClone(buffer, { transfer: [buffer] }),
    gc: () => globalThis.gc?.(),
    agent: {},
  };
  global.$262 = $262;
  global.print = (...values) => onPrint(values.join(' '));
  return {
    host: $262, script,
    async module(source) {
      if (typeof vm.SourceTextModule !== 'function') unsupported('oracle needs vm.SourceTextModule');
      const entry = { module: new vm.SourceTextModule(source, {
        context, identifier: referrer,
        initializeImportMeta(meta) { meta.url = referrer; },
        importModuleDynamically: dynamicImport,
      }), evaluation: null };
      modules.set(referrer, entry);
      await evaluate(entry);
    },
    missingCapability: () => missingCapability,
  };
}

async function runMode(relative, source, meta, strict) {
  let done;
  const completion = new Promise((resolve) => { done = resolve; });
  const instance = realm((line) => {
    if (line === ASYNC_COMPLETE) done(null);
    else if (line.startsWith(ASYNC_FAILURE_PREFIX)) done(new Error(line.slice(ASYNC_FAILURE_PREFIX.length)));
    else console.log(line);
  }, pathToFileURL(path.join(SUITE, 'test', relative)).href);
  const harness = meta.flags.includes('raw')
    ? []
    : [...DEFAULT_HARNESS, ...(meta.flags.includes('async') ? [ASYNC_HARNESS] : []), ...meta.includes];
  let timer;
  let phase = 'module evaluation';
  const timeout = new Promise((resolve) => {
    timer = setTimeout(() => resolve(new Error(`${phase} did not complete`)), ASYNC_DONE_TIMEOUT_MS);
  });
  try {
    for (const file of harness) {
      instance.script(fs.readFileSync(path.join(SUITE, 'harness', file), 'utf8'), file);
    }
    if (meta.flags.includes('module')) {
      const error = await Promise.race([instance.module(source).then(() => null), timeout]);
      if (error) throw error;
    }
    else instance.script((strict ? '"use strict";\n' : '') + source);
    if (meta.flags.includes('async')) {
      phase = '$DONE';
      const error = await Promise.race([completion, timeout]);
      if (error) throw error;
    }
    if (instance.missingCapability()) return { unverifiable: instance.missingCapability() };
    return meta.negativeType ? `expected ${meta.negativeType}` : null;
  } catch (error) {
    if (instance.missingCapability()) return { unverifiable: instance.missingCapability() };
    const name = error?.constructor?.name;
    if (meta.negativeType && name === meta.negativeType) return null;
    return error?.message === undefined ? String(error) : `${name}: ${error.message}`;
  } finally {
    clearTimeout(timer);
  }
}

// Derive both all-variant and single-variant execution from the same metadata.
function declaredVariants(meta) {
  if (meta.flags.includes('module')) return ['module'];
  if (meta.flags.includes('raw')) return ['raw'];
  if (meta.flags.includes('onlyStrict')) return ['strict'];
  if (meta.flags.includes('noStrict')) return ['sloppy'];
  return ['sloppy', 'strict'];
}

async function caseResults(relative, selected) {
  const source = fs.readFileSync(path.join(SUITE, 'test', relative), 'utf8');
  const meta = metadata(source);
  const variants = declaredVariants(meta);
  if (selected !== undefined && !variants.includes(selected)) throw new Error('variant is not declared by test metadata');
  if ((meta.flags.includes('module') || meta.features.includes('dynamic-import')) && typeof vm.SourceTextModule !== 'function') return { unverifiable: 'module cases need vm.SourceTextModule (Node: --experimental-vm-modules)' };
  const results = [];
  for (const variant of variants.filter(variant => selected === undefined || variant === selected)) {
    const failure = await runMode(relative, source, meta, variant === 'module' || variant === 'strict');
    if (failure?.unverifiable) return failure;
    results.push({ variant, failure });
  }
  return results;
}

// Preserve the historical all-mode failure shape used by waiver adjudication.
async function caseFailures(relative) {
  const results = await caseResults(relative);
  if (!Array.isArray(results)) return results;
  const failures = results.filter(result => result.failure !== null)
    .map(result => `${result.variant === 'raw' ? 'sloppy' : result.variant}: ${result.failure}`);
  return failures.length === 0 ? null : { failures };
}

async function caseVariantFailure(relative, variant) {
  const results = await caseResults(relative, variant);
  return Array.isArray(results) ? results[0].failure : results;
}

module.exports = { REPOSITORY, SUITE, caseFailures, caseVariantFailure };

if (require.main === module) {
  const [relative, variant, ...extra] = process.argv.slice(2);
  if (!relative || !variant || extra.length) {
    console.error('usage: test262-js-host.cjs RELATIVE_TEST VARIANT');
    process.exitCode = 2;
  } else caseVariantFailure(relative, variant).then(failure => {
    if (failure !== null) console.error(typeof failure === 'string' ? failure : JSON.stringify(failure));
    process.exitCode = failure === null ? 0 : failure?.unverifiable ? 2 : 1;
  }).catch(error => { console.error(error.message); process.exitCode = 2; });
}
