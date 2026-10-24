//! Compatibility surface for Node's `vm` module.
//!
//! Each context is a runtime realm with its own ECMAScript intrinsics. Scripts
//! run as global Scripts of their realm, so top-level declarations persist
//! across evaluations as in Node. Node intercepts every global access and
//! forwards it to the sandbox; here the sandbox's own properties are copied
//! onto the realm global before each evaluation and global changes copied
//! back after it, while the global's prototype forwards other names to the
//! sandbox live.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const MODULE: &str = quench_js_check::checked_js!(
    r#"((createRealm, evaluateScript) => {
  const mainGlobal = globalThis;
  const contextifyScriptSource = Symbol.for('quench.internal.contextifyScript.source');
  const contextifyScriptFilename = Symbol.for('quench.internal.contextifyScript.filename');
  // Sandbox object -> its realm: the global, its intrinsic keys, the
  // descriptors last shared between the global and the sandbox, and the
  // number of evaluations running in it.
  const realms = new WeakMap();
  const contextObjects = new Set();
  const scriptImportCallbacks = new WeakMap();
  const moduleImportCallbacks = new WeakMap();
  const dynamicImportDispatcherKey = Symbol.for('quench.internal.vm.dynamicImportDispatcher');
  const dontContextify = Symbol('vm_context_no_contextify');
  const mainContextDefaultLoader = Symbol('vm_dynamic_import_main_context_default');
  const disableProtoMode = (() => {
    const arguments = globalThis.process?.execArgv || [];
    for (let index = 0; index < arguments.length; index++) {
      const argument = arguments[index];
      if (argument === '--disable-proto=delete') return 'delete';
      if (argument === '--disable-proto=throw') return 'throw';
      if (argument === '--disable-proto') return arguments[index + 1];
    }
  })();
  function applyDisableProto(global) {
    if (disableProtoMode === 'delete') {
      Reflect.deleteProperty(global.Object.prototype, '__proto__');
    } else if (disableProtoMode === 'throw') {
      const throwProtoAccess = () => {
        const error = new global.Error('Accessing Object.prototype.__proto__ is disabled');
        error.code = 'ERR_PROTO_ACCESS';
        throw error;
      };
      Reflect.defineProperty(global.Object.prototype, '__proto__', {
        get: throwProtoAccess,
        set: throwProtoAccess,
        configurable: true,
        enumerable: false,
      });
    }
  }
  function validateCodeGeneration(value, property = 'options.codeGeneration') {
    if (value === undefined) return;
    if (value === null || typeof value !== 'object') {
      throw invalidArgType(`The "${property}" argument must be of type object.`);
    }
    for (const name of ['strings', 'wasm']) {
      if (value[name] !== undefined && typeof value[name] !== 'boolean') {
        throw invalidArgType(`The "${property}.${name}" property must be of type boolean.`);
      }
    }
  }
  function applyCodeGeneration(global, options) {
    const codeGeneration = options?.codeGeneration;
    validateCodeGeneration(codeGeneration);
    if (codeGeneration?.strings === false) {
      const reject = () => {
        throw new global.EvalError('Code generation from strings disallowed for this context');
      };
      const rejectConstructor = function Function() {
        throw new global.EvalError('Code generation from strings disallowed for this context');
      };
      Reflect.defineProperty(global, 'eval', {
        value: reject, writable: true, enumerable: false, configurable: true,
      });
      Reflect.defineProperty(global, 'Function', {
        value: rejectConstructor, writable: true, enumerable: false, configurable: true,
      });
    }
    if (codeGeneration?.wasm === false && global.WebAssembly?.Module) {
      const original = global.WebAssembly.Module;
      const reject = function Module() {
        throw new global.WebAssembly.CompileError('WebAssembly.Module(): Wasm code generation disallowed by embedder');
      };
      reject.prototype = original.prototype;
      Reflect.defineProperty(global.WebAssembly, 'Module', {
        value: reject, writable: true, enumerable: false, configurable: true,
      });
    }
  }
  let warnedMeasureMemory = false;
  function invalidArgType(message) {
    const error = new TypeError(message);
    error.code = 'ERR_INVALID_ARG_TYPE';
    return error;
  }
  function validateScriptOptions(options) {
    if (options === null || (typeof options !== 'object' && typeof options !== 'function')) {
      throw invalidArgType('The "options" argument must be of type object.');
    }
    for (const property of ['lineOffset', 'columnOffset']) {
      const value = options[property];
      if (value !== undefined && typeof value !== 'number') {
        throw invalidArgType(`The "options.${property}" property must be of type number.`);
      }
      if (value !== undefined && (!Number.isInteger(value) || value < -2147483648 || value > 2147483647)) {
        const error = new RangeError(`The value of "options.${property}" is out of range.`);
        error.code = 'ERR_OUT_OF_RANGE';
        throw error;
      }
    }
    if (options.filename !== undefined && typeof options.filename !== 'string') {
      throw invalidArgType('The "options.filename" property must be of type string.');
    }
    if (options.produceCachedData !== undefined && typeof options.produceCachedData !== 'boolean') {
      throw invalidArgType('The "options.produceCachedData" property must be of type boolean.');
    }
    if (options.cachedData !== undefined && !ArrayBuffer.isView(options.cachedData)) {
      throw invalidArgType('The "options.cachedData" property must be an instance of Buffer, TypedArray, or DataView.');
    }
      if (options.importModuleDynamically !== undefined &&
          typeof options.importModuleDynamically !== 'function' &&
          options.importModuleDynamically !== mainContextDefaultLoader) {
        throw invalidArgType('The "options.importModuleDynamically" property must be of type function.');
      }
  }
  function validateRunOptions(options) {
    if (typeof options === 'string' || options === undefined) return;
    if (options === null || typeof options !== 'object') {
      throw invalidArgType('The "options" argument must be of type object.');
    }
    for (const property of ['displayErrors', 'breakOnSigint']) {
      if (options[property] !== undefined && typeof options[property] !== 'boolean') {
        throw invalidArgType(`The "options.${property}" property must be of type boolean.`);
      }
    }
    if (options.timeout !== undefined) {
      if (typeof options.timeout !== 'number') {
        throw invalidArgType('The "options.timeout" property must be of type number.');
      }
      if (!Number.isFinite(options.timeout) || options.timeout <= 0) {
        const error = new RangeError('The value of "options.timeout" is out of range.');
        error.code = 'ERR_OUT_OF_RANGE';
        throw error;
      }
    }
  }
  const Script = class Script {
    constructor(code, options = {}) {
      validateScriptOptions(options);
      if (typeof code !== 'string') code = String(code);
      this.code = code;
      this.filename = options.filename || 'evalmachine.<anonymous>';
      this.lineOffset = options.lineOffset || 0;
      this.columnOffset = options.columnOffset || 0;
      this.sourceMapURL = extractSourceMapURL(code);
      scriptImportCallbacks.set(this, options.importModuleDynamically);
      if (options.produceCachedData) {
        this.cachedDataProduced = true;
        this.cachedData = Buffer.from(code);
      } else if (options.cachedData !== undefined) {
        const cachedLength = options.cachedData.byteLength ?? options.cachedData.length;
        this.cachedDataRejected = cachedLength !== Buffer.from(code).length;
      }
    }
    createCachedData() { return Buffer.from(this.code); }
    runInThisContext(options) { return runInThisContext(this, scriptOptions(this, options)); }
    runInContext(context, options) {
      return runInContext(this, context, scriptOptions(this, options));
    }
    runInNewContext(context, options) {
      return this.runInContext(createRunContext(context, options), options);
    }
  };
  // vm.Module is an abstract base. Keep the first-class module APIs on the
  // vm surface even when the host doesn't expose its internal module record
  // machinery. Import/export linking is deliberately handled only by the
  // runtime's module evaluator. This lightweight record supports the
  // import-free SourceTextModule subset used with vm contexts.
  class Module {
    constructor() {
      if (new.target === Module) throw new TypeError('Module is not a constructor');
    }
  }
  const moduleCounters = new WeakMap();
  const sourceModules = new WeakSet();
  const syntheticModules = new WeakSet();
  const inspectCustom = Symbol.for('nodejs.util.inspect.custom');
  function sourceExportNames(sourceText) {
    const names = new Set();
    for (const match of sourceText.matchAll(/\bexport\s+(?:async\s+)?(?:const|let|var|function|class)\s+([\w$]+)/g)) {
      names.add(match[1]);
    }
    if (/\bexport\s+default\b/.test(sourceText)) names.add('default');
    for (const match of sourceText.matchAll(/\bexport\s*\{([^}]*)\}/g)) {
      for (const item of match[1].split(',')) {
        const parts = item.trim().split(/\s+as\s+/);
        if (parts[0]) names.add(parts[1] || parts[0]);
      }
    }
    return [...names].sort((left, right) => {
      if (left === 'default') return right === 'default' ? 0 : -1;
      if (right === 'default') return 1;
      return left.localeCompare(right);
    });
  }
  function sourceModuleRequests(sourceText) {
    const requests = [];
    const seen = new Set();
    const pattern = /\b(?:import\s+(?:source\s+)?(?:[^'";]*?\s+from\s+)?|export\s+(?:\*|\{[^}]*\})\s+from\s*)['"]([^'"]+)['"](?:\s+with\s*\{([^}]*)\})?/g;
    for (const match of sourceText.matchAll(pattern)) {
      const attributes = Object.create(null);
      for (const attribute of (match[2] || '').matchAll(/(?:^|,)\s*([\w$]+)\s*:\s*(['"])(.*?)\2\s*/g)) {
        attributes[attribute[1]] = attribute[3];
      }
      const phase = /^\s*import\s+source\b/.test(match[0]) ? 'source' : 'evaluation';
      const identity = JSON.stringify([
        match[1], Object.entries(attributes).sort(([left], [right]) => left.localeCompare(right)), phase,
      ]);
      if (seen.has(identity)) continue;
      seen.add(identity);
      Object.freeze(attributes);
      requests.push(Object.freeze({ specifier: match[1], attributes, phase }));
    }
    return requests;
  }
  function vmModuleLinkFailure(message = 'Module is not linked') {
    const error = new Error(message);
    error.code = 'ERR_VM_MODULE_LINK_FAILURE';
    return error;
  }
  function moduleIdentifier(context) {
    if ((typeof context !== 'object' && typeof context !== 'function') || context === null) {
      const id = moduleCounters.get(mainGlobal) || 0;
      moduleCounters.set(mainGlobal, id + 1);
      return `vm:module(${id})`;
    }
    const id = moduleCounters.get(context) || 0;
    moduleCounters.set(context, id + 1);
    return `vm:module(${id})`;
  }
  class SourceTextModule extends Module {
    constructor(sourceText, options = {}) {
      super();
      if (typeof sourceText !== 'string') sourceText = String(sourceText);
      if (options === null || typeof options !== 'object') {
        throw invalidArgType('The "options" argument must be of type object.');
      }
      const context = options.context;
      if (context !== undefined && !isContext(context)) {
        throw invalidArgType('The "options.context" property must be an vm.Context.');
      }
      if (options.cachedData !== undefined) {
        if (!ArrayBuffer.isView(options.cachedData)) {
          throw invalidArgType('The "options.cachedData" property must be an instance of Buffer, TypedArray, or DataView.');
        }
        if (Buffer.from(options.cachedData).toString('hex') !== Buffer.from(sourceText).toString('hex')) {
          const error = new TypeError('cachedData buffer was rejected');
          error.code = 'ERR_VM_MODULE_CACHED_DATA_REJECTED';
          throw error;
        }
      }
      const namespace = Object.create(null);
      const exports = new Map();
      for (const name of sourceExportNames(sourceText)) {
        Object.defineProperty(namespace, name, { enumerable: true, get() { return exports.get(name); } });
      }
      Object.defineProperty(namespace, Symbol.toStringTag, { value: 'Module', enumerable: true });
      const moduleRequests = sourceModuleRequests(sourceText);
      Object.defineProperties(this, {
        _sourceText: { value: sourceText },
        _exports: { value: exports },
        _context: { value: context },
        _status: { value: 'unlinked', writable: true },
        error: { value: undefined, writable: true },
        identifier: { value: options.identifier || moduleIdentifier(context), enumerable: true },
        moduleRequests: { value: Object.freeze(moduleRequests) },
        dependencySpecifiers: { value: Object.freeze(moduleRequests.map(({ specifier }) => specifier)) },
        namespace: { value: namespace },
      });
      moduleImportCallbacks.set(this, options.importModuleDynamically);
      sourceModules.add(this);
    }
    createCachedData() {
      if (!sourceModules.has(this)) {
        const error = new TypeError('SourceTextModule.prototype.createCachedData called on incompatible receiver');
        error.code = 'ERR_INVALID_THIS';
        throw error;
      }
      if (this._status !== 'unlinked') {
        const error = new Error('Cached data cannot be created for a module that has already been linked or evaluated');
        error.code = 'ERR_VM_MODULE_CANNOT_CREATE_CACHED_DATA';
        throw error;
      }
      return Buffer.from(this._sourceText);
    }
    get status() { return this._status; }
    linkRequests(requests) {
      if (!sourceModules.has(this)) {
        const error = new TypeError('SourceTextModule.prototype.linkRequests called on incompatible receiver');
        error.code = 'ERR_INVALID_THIS';
        throw error;
      }
      if (this._status !== 'unlinked') throw vmModuleLinkFailure('Module has already been linked');
      if (!Array.isArray(requests)) throw invalidArgType('The "requests" argument must be an array');
      if (this.moduleRequests.length !== requests.length) {
        throw vmModuleLinkFailure('The number of linked modules must match the number of module requests');
      }
      if (/\bexport\s*\*\s*from\b/.test(this._sourceText)) {
        for (const request of requests) {
          if (!request || !request.namespace) continue;
          for (const name of Reflect.ownKeys(request.namespace)) {
            if (name === 'default' || name === Symbol.toStringTag || Object.hasOwn(this.namespace, name)) continue;
            Object.defineProperty(this.namespace, name, {
              enumerable: true,
              get() { return request.namespace[name]; },
            });
          }
        }
      }
      Object.freeze(this.namespace);
      this._status = 'linked';
    }
    instantiate() {
      if (!sourceModules.has(this)) {
        const error = new TypeError('SourceTextModule.prototype.instantiate called on incompatible receiver');
        error.code = 'ERR_INVALID_THIS';
        throw error;
      }
      if (this._status === 'unlinked') throw vmModuleLinkFailure();
      if (this._status === 'errored') throw vmModuleLinkFailure('Module is in an errored state');
    }
    link(linker) {
      if (this._status !== 'unlinked') return Promise.reject(new Error('Module has already been linked.'));
      this._status = 'linking';
      if (typeof linker !== 'function') {
        this._status = 'errored';
        return Promise.reject(invalidArgType('The "linker" argument must be of type function.'));
      }
      this._status = 'linked';
      return Promise.resolve(this);
    }
    evaluate(options = {}) {
      if (this._status !== 'linked' && this._status !== 'evaluated') {
        return Promise.reject(new Error('Module status must be linked or evaluated'));
      }
      if (this._status === 'evaluated') return Promise.resolve(undefined);
      try {
        validateRunOptions(options);
        // The host evaluator has no interruptible timeout entry point yet.
        // Reject a requested timeout instead of risking a non-terminating VM.
        if (options && options.timeout !== undefined) throw new Error('Script execution timed out');
        // The host script evaluator does not accept module declarations. The
        // static-import-free SourceTextModule subset can evaluate exported
        // declarations as a script and collect their live bindings.
        const exported = sourceExportNames(this._sourceText)
          .filter((name) => name !== 'default' && /^[\w$]+$/.test(name));
        const hasDefaultExport = /\bexport\s+default\b/.test(this._sourceText);
        const body = this._sourceText
          .replace(/\bexport\s+(?=(?:async\s+)?(?:const|let|var|function|class)\b)/g, '')
          .replace(/\bexport\s+default\s+/g, 'const __quenchDefaultExport = ')
          .replace(/\bexport\s*\{[^}]*\}\s*;?/g, '');
        // SourceTextModule uses module grammar, including top-level await.
        // The host currently exposes a Script evaluator, so run the supported
        // supported module subset inside an async function and return its
        // exported bindings after the body settles.
        const exportsObject = exported.length === 0 && !hasDefaultExport
          ? 'undefined'
          : `({ ${exported.join(', ')}${hasDefaultExport ? `${exported.length ? ', ' : ''}default: __quenchDefaultExport` : ''} })`;
        const isAsync = /\bawait\b/.test(body);
        const code = `(${isAsync ? 'async ' : ''}function() {\n'use strict';\n${body}\n;return (${exportsObject});\n})()`;
        let namespaceValues;
        if (this._context === undefined) {
          namespaceValues = runInThisContext(code, {
            filename: this.identifier,
            importModuleDynamically: moduleImportCallbacks.get(this),
            dynamicImportReferrer: this,
            ...options,
          });
        } else {
          namespaceValues = runInContext(code, this._context, {
            filename: this.identifier,
            importModuleDynamically: moduleImportCallbacks.get(this),
            dynamicImportReferrer: this,
            ...options,
          });
        }
        const complete = (values) => {
          if (values && typeof values === 'object') {
            for (const name of exported) this._exports.set(name, values[name]);
            if (hasDefaultExport) this._exports.set('default', values.default);
          }
          this._status = 'evaluated';
          return undefined;
        };
        const fail = (error) => {
          this._status = 'errored';
          this.error = error;
          throw error;
        };
        if (!isAsync) {
          try {
            complete(namespaceValues);
            return Promise.resolve(undefined);
          } catch (error) {
            this._status = 'errored';
            this.error = error;
            return Promise.reject(error);
          }
        }
        return Promise.resolve(namespaceValues).then(complete, fail);
      } catch (error) {
        this._status = 'errored';
        this.error = error;
        return Promise.reject(error);
      }
    }
    [inspectCustom](depth, options, inspect) {
      if (!sourceModules.has(this)) {
        const error = new TypeError('Cannot inspect SourceTextModule from an incompatible receiver');
        error.code = 'ERR_INVALID_THIS';
        throw error;
      }
      if (depth < 0) return '[SourceTextModule]';
      return `SourceTextModule {\n  status: ${inspect(this._status)},\n  identifier: ${inspect(this.identifier)},\n  context: ${inspect(this._context, options)}\n}`;
    }
  }
  class SyntheticModule extends Module {
    constructor(exportNames, evaluationSteps, options = {}) {
      super();
      if (!Array.isArray(exportNames)) {
        const received = exportNames === undefined ? 'undefined' :
          exportNames === null ? 'null' : `type ${typeof exportNames} (${String(exportNames)})`;
        throw new TypeError(`The "exportNames" argument must be an Array of unique strings. Received ${received}`);
      }
      const duplicate = exportNames.find((name, index) => exportNames.indexOf(name) !== index);
      if (duplicate !== undefined) {
        throw new TypeError(`The property 'exportNames.${duplicate}' is duplicated. Received '${duplicate}'`);
      }
      if (exportNames.some((name) => typeof name !== 'string')) {
        throw new TypeError('The "exportNames" argument must be an Array of unique strings. Received an instance of Array');
      }
      if (typeof evaluationSteps !== 'function') {
        const received = evaluationSteps === undefined ? 'undefined' :
          evaluationSteps === null ? 'null' : `type ${typeof evaluationSteps} (${String(evaluationSteps)})`;
        throw new TypeError(`The "evaluateCallback" argument must be of type function. Received ${received}`);
      }
      if (options === null || typeof options !== 'object') {
        const received = options === null ? 'null' : `type ${typeof options} (${String(options)})`;
        throw new TypeError(`The "options" argument must be of type object. Received ${received}`);
      }
      const context = options.context;
      if (context !== undefined && !isContext(context)) {
        throw invalidArgType('The "options.context" property must be an vm.Context.');
      }
      const exports = new Map(exportNames.map((name) => [name, undefined]));
      const namespace = Object.create(null);
      for (const name of exportNames) {
        Object.defineProperty(namespace, name, {
          enumerable: true,
          get() { return exports.get(name); },
        });
      }
      Object.defineProperties(this, {
        _context: { value: context },
        _evaluationSteps: { value: evaluationSteps },
        _exports: { value: exports },
        _status: { value: 'linked', writable: true },
        error: { value: undefined, writable: true },
        identifier: { value: options.identifier || moduleIdentifier(context), enumerable: true },
        namespace: { value: Object.freeze(namespace) },
      });
      syntheticModules.add(this);
    }
    get status() { return this._status; }
    setExport(name, value) {
      if (!syntheticModules.has(this)) {
        const error = new TypeError('SyntheticModule.prototype.setExport called on incompatible receiver');
        error.code = 'ERR_INVALID_THIS';
        throw error;
      }
      if (typeof name !== 'string') throw new TypeError('Export name must be a string');
      if (!this._exports.has(name)) throw new ReferenceError(`Export '${name}' is not defined`);
      this._exports.set(name, value);
    }
    link() { return undefined; }
    evaluate() {
      if (this._status === 'evaluated') return Promise.resolve(undefined);
      if (this._status !== 'linked') return Promise.reject(new Error('Module status must be linked or evaluated'));
      try {
        const result = this._evaluationSteps.call(this);
        this._status = 'evaluated';
        if (result !== null && (typeof result === 'object' || typeof result === 'function') &&
            typeof result.then === 'function') {
          Promise.resolve(result).catch((error) => {
            this._status = 'errored';
            this.error = error;
          });
        }
        return Promise.resolve(undefined);
      } catch (error) {
        this._status = 'errored';
        this.error = error;
        return Promise.reject(error);
      }
    }
    [inspectCustom](depth, options, inspect) {
      if (!syntheticModules.has(this)) {
        const error = new TypeError('Cannot inspect SyntheticModule from an incompatible receiver');
        error.code = 'ERR_INVALID_THIS';
        throw error;
      }
      if (depth < 0) return '[SyntheticModule]';
      return `SyntheticModule {\n  status: ${inspect(this._status)},\n  identifier: ${inspect(this.identifier)},\n  context: ${inspect(this._context, options)}\n}`;
    }
  }
  function scriptOptions(script, options) {
    if (typeof options === 'string') {
      throw invalidArgType('The "options" argument must be of type object.');
    }
    validateRunOptions(options);
    return {
      filename: script.filename,
      importModuleDynamically: scriptImportCallbacks.get(script),
      dynamicImportReferrer: script,
      ...(typeof options === 'object' ? options : {}),
    };
  }
  function dynamicImportFailureCode(options) {
    if (options?.importModuleDynamically === mainContextDefaultLoader) return undefined;
    if (typeof options?.importModuleDynamically !== 'function') {
      return 'ERR_VM_DYNAMIC_IMPORT_CALLBACK_MISSING';
    }
    if (!globalThis.process?.execArgv?.includes('--experimental-vm-modules')) {
      return 'ERR_VM_DYNAMIC_IMPORT_CALLBACK_MISSING_FLAG';
    }
  }
  function rejectDynamicImports(source, options) {
    const code = dynamicImportFailureCode(options);
    if (code === undefined || !/\bimport\s*\(/.test(source)) return source;
    const reason = `Object.assign(new TypeError('A dynamic import callback was not specified.'), { code: '${code}' })`;
    return source.replace(/\bimport\s*\([^()]*\)/g, `Promise.reject(${reason})`);
  }
  function rewriteDynamicImports(source) {
    const prefix = '((__quenchImport)=>Promise.resolve().then(()=>__quenchImport(';
    const suffix = `)))(globalThis[Symbol.for('quench.internal.vm.dynamicImportDispatcher')])`;
    let output = '';
    let cursor = 0;
    while (cursor < source.length) {
      const match = /\bimport\s*\(/g;
      match.lastIndex = cursor;
      const found = match.exec(source);
      if (!found) break;
      const open = source.indexOf('(', found.index);
      let depth = 1;
      let quote;
      let escaped = false;
      let close = -1;
      for (let index = open + 1; index < source.length; index++) {
        const character = source[index];
        if (quote !== undefined) {
          if (escaped) escaped = false;
          else if (character === '\\') escaped = true;
          else if (character === quote) quote = undefined;
          continue;
        }
        if (character === '"' || character === "'" || character === '`') {
          quote = character;
        } else if (character === '(') {
          depth++;
        } else if (character === ')' && --depth === 0) {
          close = index;
          break;
        }
      }
      if (close < 0) break;
      output += source.slice(cursor, found.index) + prefix + source.slice(open + 1, close) + suffix;
      cursor = close + 1;
    }
    return output + source.slice(cursor);
  }
  function dynamicImportDispatcher(options) {
    const callback = options?.importModuleDynamically;
    const referrer = options?.dynamicImportReferrer;
    return (specifier, importOptions) => {
      if (callback === mainContextDefaultLoader) {
        return import(specifier);
      }
      let attributes = Object.create(null);
      const supplied = importOptions?.with ?? importOptions?.assert;
      if (supplied !== undefined && supplied !== null) {
        for (const key of Reflect.ownKeys(Object(supplied))) {
          if (typeof key === 'string') attributes[key] = supplied[key];
        }
      }
      let module;
      try {
        module = callback(specifier, referrer, attributes, 'evaluation');
      } catch (error) {
        return Promise.reject(error);
      }
      return Promise.resolve(module).then((record) => {
        if (!sourceModules.has(record) && !syntheticModules.has(record)) {
          const error = new TypeError('The callback must return a Module instance');
          error.code = 'ERR_VM_MODULE_NOT_MODULE';
          throw error;
        }
        return record.namespace;
      });
    };
  }
  function prepareDynamicImports(source, options) {
    const code = dynamicImportFailureCode(options);
    if (!/\bimport\s*\(/.test(source)) return source;
    if (code !== undefined) return rejectDynamicImports(source, options);
    return rewriteDynamicImports(source);
  }
  function withDynamicImportDispatcher(global, options, callback) {
    const importCallback = options?.importModuleDynamically;
    if (typeof importCallback !== 'function' && importCallback !== mainContextDefaultLoader) return callback();
    const key = dynamicImportDispatcherKey;
    const prior = Reflect.getOwnPropertyDescriptor(global, key);
    Reflect.defineProperty(global, key, {
      value: dynamicImportDispatcher(options), configurable: true,
    });
    try { return callback(); }
    finally {
      if (prior) Reflect.defineProperty(global, key, prior);
      else Reflect.deleteProperty(global, key);
    }
  }
  function filenameOf(options, code) {
    const filename = typeof options === 'string' ? options : options?.filename;
    if (filename) return filename;
    if (code !== null && typeof code === 'object' &&
        typeof code[contextifyScriptFilename] === 'string') {
      return code[contextifyScriptFilename];
    }
    return 'evalmachine.<anonymous>';
  }
  function extractSourceMapURL(source) {
    let sourceMapURL;
    const directive = /^[\t ]*\/\/[#@][\t ]*sourceMappingURL[\t ]*=[\t ]*([^\s]+)[\t ]*$/;
    for (const line of source.split(/\r?\n/)) {
      const match = directive.exec(line);
      if (match) sourceMapURL = match[1];
    }
    return sourceMapURL;
  }
  function createContext(context = {}, options = {}) {
    const noContextify = context === dontContextify;
    if (!noContextify && (context === null || (typeof context !== 'object' && typeof context !== 'function'))) {
      throw invalidArgType('The "contextObject" argument must be of type object.');
    }
    if (options === null || typeof options !== 'object') {
      const received = options === null ? 'Received null' :
        `Received type ${typeof options} (${typeof options === 'string' ? `'${options}'` : String(options)})`;
      const error = new TypeError(`The "options" argument must be of type object. ${received}`);
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    for (const property of ['name', 'origin']) {
      if (options[property] !== undefined && typeof options[property] !== 'string') {
        const received = options[property] === null ? 'Received null' :
          `Received type ${typeof options[property]} (${String(options[property])})`;
        const error = new TypeError(`The "options.${property}" property must be of type string. ${received}`);
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
    }
    if (noContextify) {
      const global = createRealm();
      applyDisableProto(global);
      applyCodeGeneration(global, options);
      removeInternalRealmBindings(global);
      const realm = { global, intrinsic: new Set(Reflect.ownKeys(global)), forwarded: new Map(), depth: 0,
        stringsDisabled: options.codeGeneration?.strings === false,
        importModuleDynamically: options.importModuleDynamically };
      realms.set(global, realm);
      contextObjects.add(global);
      return global;
    }
    if (!realms.has(context)) {
      const global = createRealm();
      applyDisableProto(global);
      applyCodeGeneration(global, options);
      removeInternalRealmBindings(global);
      const realm = { global, intrinsic: new Set(Reflect.ownKeys(global)), forwarded: new Map(), depth: 0,
        stringsDisabled: options.codeGeneration?.strings === false,
        importModuleDynamically: options.importModuleDynamically };
      Reflect.setPrototypeOf(global, sandboxScope(context, realm));
      installContextEval(context, realm);
      realms.set(context, realm);
      contextObjects.add(context);
    }
    return context;
  }
  // These constructor functions are intrinsic implementation details in V8,
  // not own global bindings in Node vm contexts.
  function removeInternalRealmBindings(global) {
    for (const name of ['TypedArray', 'AsyncFunction', 'GeneratorFunction', 'AsyncGeneratorFunction']) {
      Reflect.deleteProperty(global, name);
    }
  }
  function installContextEval(context, realm) {
    const originalEval = Reflect.get(realm.global, 'eval');
    if (typeof originalEval !== 'function') return;
    const contextEval = function(source) {
      if (typeof source === 'string') {
        if (realm.stringsDisabled) {
          throw new realm.global.EvalError('Code generation from strings disallowed for this context');
        }
        const match = /^\s*import\s*\(\s*(['"])([^'"]+)\1\s*\)\s*;?\s*$/.exec(source);
        if (match) {
          const callback = realm.importModuleDynamically;
          if (typeof callback !== 'function') {
            const error = new TypeError('A dynamic import callback was not specified.');
            error.code = 'ERR_VM_DYNAMIC_IMPORT_CALLBACK_MISSING';
            return Promise.reject(error);
          }
          try {
            return Promise.resolve(callback(match[2], context)).then((module) => module.namespace);
          } catch (error) {
            return Promise.reject(error);
          }
        }
        return evaluateScript(realm.global, source, 'evalmachine.<anonymous>');
      }
      return source;
    };
    Reflect.defineProperty(realm.global, 'eval', {
      value: contextEval, writable: true, enumerable: false, configurable: true,
    });
  }
  // Names the realm global does not own resolve through its prototype, which
  // reads the sandbox with its prototype chain. A new name written by guest
  // code becomes a global property; outside an evaluation (timers, promise
  // jobs) it also reaches the sandbox at once, during one it is copied back
  // with the evaluation's other global changes.
  function sandboxScope(context, realm) {
    const { global } = realm;
    return new Proxy(Object.create(Reflect.getPrototypeOf(global)), {
      get(target, key, receiver) {
        // A property copied to the sandbox can be deleted from the realm's
        // global while guest code is still running. Observe that deletion
        // lazily here so an immediate subsequent read doesn't resurrect the
        // stale sandbox value through this prototype proxy.
        if (!Reflect.getOwnPropertyDescriptor(global, key)) {
          const forwarded = realm.forwarded.get(key);
          const current = Reflect.getOwnPropertyDescriptor(context, key);
          if (forwarded && current && sameDescriptor(current, forwarded)) {
            Reflect.deleteProperty(context, key);
            realm.forwarded.delete(key);
          }
        }
        if (key in context) {
          const value = Reflect.get(context, key);
          // Node's contextified global is the identity for self-references in
          // the sandbox (for example, `sandbox.window = sandbox`).
          return value === context ? global : value;
        }
        return Reflect.get(target, key, receiver);
      },
      set(target, key, value, receiver) {
        if (receiver !== global) return Reflect.set(target, key, value, receiver);
        const sandboxDescriptor = Reflect.getOwnPropertyDescriptor(context, key);
        if (sandboxDescriptor && !('value' in sandboxDescriptor)) {
          // Preserve sandbox accessors (such as REPL's `_` setter) when a
          // guest global write is forwarded through this scope proxy.
          return Reflect.set(context, key, value);
        }
        const descriptor = { value, writable: true, enumerable: true, configurable: true };
        if (!Reflect.defineProperty(global, key, descriptor)) return false;
        if (realm.depth === 0) {
          Reflect.set(context, key, value);
          realm.forwarded.set(key, descriptor);
        }
        return true;
      },
      has(target, key) { return key in context || Reflect.has(target, key); },
    });
  }
  function isContext(context) {
    if (context === null || (typeof context !== 'object' && typeof context !== 'function')) {
      throw invalidArgType('The "context" argument must be an object.');
    }
    return realms.has(context);
  }
  // Own sandbox properties are own properties of the global, with the same descriptors.
  function forwardSandbox(context, realm) {
    for (const [key, descriptor] of realm.forwarded) {
      if (!Reflect.getOwnPropertyDescriptor(context, key) &&
          sameDescriptor(Reflect.getOwnPropertyDescriptor(realm.global, key), descriptor)) {
        Reflect.deleteProperty(realm.global, key);
        realm.forwarded.delete(key);
      }
    }
    for (const key of Reflect.ownKeys(context)) {
      const sandboxDescriptor = Reflect.getOwnPropertyDescriptor(context, key);
      // Node's contextified global proxy is used for self references inside
      // the realm. Preserve the sandbox's identity outside the realm while
      // making `sandbox.self = sandbox` resolve to the realm global in code.
      const descriptor = 'value' in sandboxDescriptor && sandboxDescriptor.value === context
        ? { ...sandboxDescriptor, value: realm.global }
        : sandboxDescriptor;
      const own = Reflect.getOwnPropertyDescriptor(realm.global, key);
      if (own && !own.configurable) continue;
      if (own && !realm.intrinsic.has(key) && !realm.forwarded.has(key)) continue;
      if (Reflect.defineProperty(realm.global, key, descriptor)) realm.forwarded.set(key, descriptor);
    }
  }
  // Global properties the script created or changed become sandbox properties.
  function reflectGlobals(context, realm) {
    for (const key of Reflect.ownKeys(realm.global)) {
      if (realm.intrinsic.has(key) && !realm.forwarded.has(key)) continue;
      const descriptor = Reflect.getOwnPropertyDescriptor(realm.global, key);
      if (sameDescriptor(descriptor, realm.forwarded.get(key))) continue;
      const sandboxDescriptor = 'value' in descriptor && descriptor.value === realm.global
        ? { ...descriptor, value: context }
        : descriptor;
      const current = Reflect.getOwnPropertyDescriptor(context, key);
      if (current && 'value' in sandboxDescriptor && 'value' in current &&
          sandboxDescriptor.enumerable === current.enumerable && sandboxDescriptor.writable === current.writable &&
          sandboxDescriptor.configurable === current.configurable) {
        context[key] = sandboxDescriptor.value;
      } else {
        Reflect.defineProperty(context, key, sandboxDescriptor);
      }
      realm.forwarded.set(key, descriptor);
    }
    // A context global can be deleted from inside the guest realm. Mirror that
    // removal back to the sandbox so `in`, own-key queries, and a later
    // evaluation all observe the same property state.
    for (const [key, descriptor] of realm.forwarded) {
      if (Reflect.getOwnPropertyDescriptor(realm.global, key) !== undefined) continue;
      const current = Reflect.getOwnPropertyDescriptor(context, key);
      if (current && sameDescriptor(current, descriptor)) Reflect.deleteProperty(context, key);
      realm.forwarded.delete(key);
    }
  }
  function sameDescriptor(left, right) {
    if (left === undefined || right === undefined) return left === right;
    return Object.is(left.value, right.value) && left.get === right.get && left.set === right.set &&
      left.writable === right.writable && left.enumerable === right.enumerable &&
      left.configurable === right.configurable;
  }
  function addFilename(error, options, source) {
    if (error && typeof error === 'object' && typeof error.stack === 'string') {
      const filename = typeof options === 'string' ? options : options?.filename;
      if (filename && !error.stack.startsWith(`${filename}:`)) {
        error.stack = `${filename}:1\n${error.stack}`;
      }
      if (filename) {
        const marker = `at ${filename}:`;
        const frame = error.stack.indexOf(marker);
        if (frame >= 0) {
          const position = frame + marker.length;
          const end = error.stack.indexOf('\n', position);
          const location = error.stack.slice(position, end < 0 ? undefined : end).split(':');
          const lineOffset = options?.lineOffset || 0;
          const columnOffset = options?.columnOffset || 0;
          const line = Number(location[0]) + lineOffset;
          const column = Number(location[1]) + columnOffset;
          const headerEnd = error.stack.indexOf('\n');
          const header = `${filename}:${line}\n${String(source).split('\n', 1)[0]}\n ^`;
          error.stack = header + error.stack.slice(headerEnd);
          error.stack = error.stack.replace(`${marker}${location[0]}:${location[1]}`, `${marker}${line}:${column}`);
        }
      }
    }
  }
  function sourceOf(code) {
    if (typeof code === 'object' && code !== null && typeof code.code === 'string') return code.code;
    if (typeof code === 'object' && code !== null && typeof code[contextifyScriptSource] === 'string') {
      return code[contextifyScriptSource];
    }
    return String(code);
  }
  function runInThisContext(code, options) {
    validateRunOptions(options);
    const source = prepareDynamicImports(sourceOf(code), options);
    try {
      return withDynamicImportDispatcher(mainGlobal, options,
        () => evaluateScript(mainGlobal, source, filenameOf(options, code)));
    }
    catch (error) { addFilename(error, options, source); throw error; }
  }
  function runInContext(code, context, options = {}) {
    validateRunOptions(options);
    if (context === null || (typeof context !== 'object' && typeof context !== 'function')) {
      throw invalidArgType('The "contextObject" argument must be of type object.');
    }
    const realm = context !== null && (typeof context === 'object' || typeof context === 'function')
      ? realms.get(context) : undefined;
    if (realm === undefined) {
      const error = new TypeError('The "contextifiedObject" argument must be an vm.Context.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    forwardSandbox(context, realm);
    realm.depth++;
    const source = prepareDynamicImports(sourceOf(code), options);
    try {
      return withDynamicImportDispatcher(realm.global, options,
        () => evaluateScript(realm.global, source, filenameOf(options, code)));
    }
    catch (error) { addFilename(error, options, source); throw error; }
    finally {
      realm.depth--;
      reflectGlobals(context, realm);
    }
  }
  function createRunContext(context = {}, options = {}) {
    validateRunOptions(options);
    if (options !== null && typeof options === 'object') {
      for (const property of ['contextName', 'contextOrigin']) {
        if (options[property] !== undefined && typeof options[property] !== 'string') {
          const received = options[property] === null ? 'Received null' :
            `Received type ${typeof options[property]} (${String(options[property])})`;
          const error = new TypeError(`The "options.${property}" property must be of type string. ${received}`);
          error.code = 'ERR_INVALID_ARG_TYPE';
          throw error;
        }
      }
      validateCodeGeneration(options.contextCodeGeneration, 'options.contextCodeGeneration');
    }
    const contextOptions = typeof options === 'string' ? {} : {
      ...options,
      codeGeneration: options.contextCodeGeneration,
    };
    return createContext(context, contextOptions);
  }
  function runInNewContext(code, context = {}, options = {}) {
    return runInContext(code, createRunContext(context, options), options);
  }
  function compileFunction(code, params = [], options = {}) {
    function received(value) {
      if (value === null) return 'Received null';
      if (typeof value === 'string') return `Received type string ('${value}')`;
      if (typeof value === 'number' || typeof value === 'boolean') return `Received type ${typeof value} (${value})`;
      if (typeof value === 'symbol') return `Received type symbol (${String(value)})`;
      if (typeof value === 'function') return `Received function ${value.name}`;
      if (typeof value === 'object') return `Received an instance of ${value.constructor?.name || 'Object'}`;
      return `Received ${String(value)}`;
    }
    function invalid(message) {
      const error = new TypeError(message);
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (typeof code !== 'string') invalid(`The "code" argument must be of type string. ${received(code)}`);
    if (!Array.isArray(params)) invalid(`The "params" argument must be an instance of Array. ${received(params)}`);
    if (options === null || typeof options !== 'object') {
      invalid(`The "options" argument must be of type object. ${received(options)}`);
    }
    for (const [property, type] of [['filename', 'string'], ['columnOffset', 'number'], ['lineOffset', 'number'], ['produceCachedData', 'boolean']]) {
      if (options[property] !== undefined && typeof options[property] !== type) {
        invalid(`The "options.${property}" property must be of type ${type}. ${received(options[property])}`);
      }
    }
    if (options.importModuleDynamically !== undefined &&
        typeof options.importModuleDynamically !== 'function' &&
        options.importModuleDynamically !== mainContextDefaultLoader) {
      invalid(`The "options.importModuleDynamically" property must be of type function. ${received(options.importModuleDynamically)}`);
    }
    if (options.cachedData !== undefined && !ArrayBuffer.isView(options.cachedData) && !(options.cachedData instanceof DataView)) {
      invalid(`The "options.cachedData" property must be an instance of Buffer, TypedArray, or DataView. ${received(options.cachedData)}`);
    }
    if (options.contextExtensions !== undefined && !Array.isArray(options.contextExtensions)) {
      invalid(`The "options.contextExtensions" property must be an instance of Array. ${received(options.contextExtensions)}`);
    }
    for (let index = 0; index < (options.contextExtensions || []).length; index++) {
      const extension = options.contextExtensions[index];
      if (extension === null || typeof extension !== 'object') {
        invalid(`The "options.contextExtensions[${index}]" property must be of type object. ${received(extension)}`);
      }
    }
    if (options.parsingContext !== undefined && !realms.has(options.parsingContext)) {
      invalid(`The "options.parsingContext" property must be an instance of Context. ${received(options.parsingContext)}`);
    }
    const names = params.map(String);
    const body = String(code);
    const dynamicImportEnabled = (typeof options.importModuleDynamically === 'function' ||
      options.importModuleDynamically === mainContextDefaultLoader) &&
      /\bimport\s*\(/.test(body) && dynamicImportFailureCode(options) === undefined;
    const failureCode = dynamicImportFailureCode(options);
    const executableBody = failureCode !== undefined && /\bimport\s*\(/.test(body)
      ? rejectDynamicImports(body, options)
      : dynamicImportEnabled
      ? `const __quenchImport = (specifier) => Promise.resolve().then(() => __quenchImportCallback(specifier, __quenchImportReferrer)).then((module) => { if (!__quenchIsModule(module)) { const error = new TypeError('Provided module is not an instance of Module'); error.code = 'ERR_VM_MODULE_NOT_MODULE'; throw error; } return module.namespace; });\n${body.replace(/\bimport\s*\(/g, '__quenchImport(')}`
      : body;
    const extensionValues = new Map();
    if (options.parsingContext !== undefined) {
      const context = options.parsingContext;
      forwardSandbox(context, realms.get(context));
      for (const key of Reflect.ownKeys(context)) {
        if (typeof key === 'string') extensionValues.set(key, context[key]);
      }
    }
    for (const extension of options.contextExtensions || []) {
      if (extension !== null && (typeof extension === 'object' || typeof extension === 'function')) {
        for (const key of Object.keys(extension)) extensionValues.set(key, extension[key]);
      }
    }
    const extensionNames = [...extensionValues.keys()].filter((key) => /^[A-Za-z_$][\w$]*$/.test(key));
    let fn;
    try {
      fn = dynamicImportEnabled
        ? Function(...extensionNames, ...names, '__quenchImportCallback', '__quenchImportReferrer', '__quenchIsModule', executableBody)
        : Function(...extensionNames, ...names, executableBody);
    }
    catch (error) {
      if (error instanceof SyntaxError && body.startsWith('});')) {
        throw new SyntaxError("Unexpected token '}'");
      }
      throw error;
    }
    if (extensionNames.length !== 0) {
      const target = fn;
      const values = extensionNames.map((key) => extensionValues.get(key));
      fn = function(...args) { return target.apply(this, [...values, ...args]); };
    }
    if (dynamicImportEnabled || extensionNames.length !== 0) {
      Object.defineProperty(fn, 'length', { value: names.length });
    }
    Object.defineProperty(fn, 'toString', {
      configurable: true,
      value() { return `function (${names.join(', ')}) {\n${body}\n}`; },
    });
    if (options.produceCachedData) {
      fn.cachedDataProduced = true;
      fn.cachedData = Buffer.from(body);
    } else if (options.cachedData !== undefined) {
      fn.cachedDataRejected = Buffer.from(options.cachedData).toString('hex') !== Buffer.from(body).toString('hex');
    }
    const filename = options.filename || '<anonymous>';
    const lineOffset = options.lineOffset || 0;
    const columnOffset = options.columnOffset || 0;
    let proxiedFn;
    proxiedFn = new Proxy(fn, {
      apply(target, thisArg, args) {
        if (dynamicImportEnabled) {
          args = [...args, options.importModuleDynamically, proxiedFn,
            (module) => sourceModules.has(module) || syntheticModules.has(module)];
        }
        try { return Reflect.apply(target, thisArg, args); }
        catch (error) {
          if (typeof error?.stack === 'string') {
            error.stack = error.stack.replace(
              /^\s*at eval \(eval at compileFunction.*?, <anonymous>:(\d+):(\d+)\)$/m,
              (_frame, line, column) => `    at ${filename}:${Math.max(1, Number(line) - 2 + lineOffset)}:${Math.max(1, (error instanceof ReferenceError ? 1 : Number(column)) + columnOffset)}`
            );
          }
          throw error;
        }
      },
    });
    return proxiedFn;
  }
  function measureMemory(options = {}) {
    if (options === null || typeof options !== 'object' || Array.isArray(options)) {
      throw invalidArgType('The "options" argument must be of type object.');
    }
    const mode = options.mode === undefined ? 'summary' : options.mode;
    if (mode !== 'summary' && mode !== 'detailed') {
      const error = new TypeError('The "options.mode" property must be one of: "summary", "detailed"');
      error.code = 'ERR_INVALID_ARG_VALUE';
      throw error;
    }
    const execution = options.execution === undefined ? 'default' : options.execution;
    if (execution !== 'default' && execution !== 'eager') {
      const error = new TypeError('The "options.execution" property must be one of: "default", "eager"');
      error.code = 'ERR_INVALID_ARG_VALUE';
      throw error;
    }
    if (!warnedMeasureMemory) {
      warnedMeasureMemory = true;
      process.emitWarning('vm.measureMemory is an experimental feature and might change at any time', { type: 'ExperimentalWarning' });
    }
    const estimate = 1;
    const measurement = () => ({ jsMemoryEstimate: estimate, jsMemoryRange: [estimate, estimate] });
    const result = { total: measurement() };
    if (mode === 'detailed') {
      result.current = measurement();
      result.other = [...contextObjects].filter((context) => realms.has(context)).map(measurement);
    }
    return Promise.resolve(result);
  }
  const constants = Object.freeze({ USE_MAIN_CONTEXT_DEFAULT_LOADER: mainContextDefaultLoader, DONT_CONTEXTIFY: dontContextify });
  return {
    Script, createScript: (code, options) => new Script(code, typeof options === 'string' ? { filename: options } : options),
    Module, SourceTextModule, SyntheticModule,
    createContext, isContext, runInThisContext, runInContext, runInNewContext,
    compileFunction, constants,
    measureMemory,
  };
})"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(MODULE, "node:vm/shared.js")?;
    let create_realm = context.host_function(crate::host::shared_vm::operation("vmCreateRealm"))?;
    let evaluate = context.host_function(crate::host::shared_vm::operation("vmEvaluateScript"))?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[create_realm, evaluate])
}

pub(crate) fn create_realm(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    context.create_realm_rooted()
}

/// `(global, source, filename)`: evaluate `source` as a global Script of `global`'s realm.
pub(crate) fn evaluate_script(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let [global, source, filename] = args else {
        return Err(RootedError::host(
            "vm script evaluation expects three arguments",
        ));
    };
    let source = context.to_string(*source)?;
    let filename = context.to_string(*filename)?;
    context.evaluate_script_in_realm_rooted(*global, &source, &filename)
}
