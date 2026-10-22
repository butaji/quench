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
  // Sandbox object -> its realm: the global, its intrinsic keys, the
  // descriptors last shared between the global and the sandbox, and the
  // number of evaluations running in it.
  const realms = new WeakMap();
  const Script = class Script {
    constructor(code, options = {}) {
      if (typeof code !== 'string') code = String(code);
      this.code = code;
      this.filename = options.filename || 'evalmachine.<anonymous>';
      this.lineOffset = options.lineOffset || 0;
      this.columnOffset = options.columnOffset || 0;
    }
    runInThisContext(options) { return runInThisContext(this.code, scriptOptions(this, options)); }
    runInContext(context, options) {
      return runInContext(this.code, context, scriptOptions(this, options));
    }
    runInNewContext(context, options) {
      return runInNewContext(this.code, context, scriptOptions(this, options));
    }
  };
  function scriptOptions(script, options) {
    return { filename: script.filename, ...(typeof options === 'object' ? options : {}) };
  }
  function filenameOf(options) {
    const filename = typeof options === 'string' ? options : options?.filename;
    return filename || 'evalmachine.<anonymous>';
  }
  function createContext(context = {}, options = {}) {
    if (context === null || (typeof context !== 'object' && typeof context !== 'function')) {
      throw new TypeError('The "contextObject" argument must be an object.');
    }
    if (!realms.has(context)) {
      const global = createRealm();
      const realm = { global, intrinsic: new Set(Reflect.ownKeys(global)), forwarded: new Map(), depth: 0 };
      Reflect.setPrototypeOf(global, sandboxScope(context, realm));
      realms.set(context, realm);
    }
    return context;
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
        return key in context ? Reflect.get(context, key) : Reflect.get(target, key, receiver);
      },
      set(target, key, value, receiver) {
        if (receiver !== global) return Reflect.set(target, key, value, receiver);
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
    return context !== null && (typeof context === 'object' || typeof context === 'function') &&
      realms.has(context);
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
      const descriptor = Reflect.getOwnPropertyDescriptor(context, key);
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
      const current = Reflect.getOwnPropertyDescriptor(context, key);
      if (current && 'value' in descriptor && 'value' in current &&
          descriptor.enumerable === current.enumerable && descriptor.writable === current.writable &&
          descriptor.configurable === current.configurable) {
        context[key] = descriptor.value;
      } else {
        Reflect.defineProperty(context, key, descriptor);
      }
      realm.forwarded.set(key, descriptor);
    }
  }
  function sameDescriptor(left, right) {
    if (left === undefined || right === undefined) return left === right;
    return Object.is(left.value, right.value) && left.get === right.get && left.set === right.set &&
      left.writable === right.writable && left.enumerable === right.enumerable &&
      left.configurable === right.configurable;
  }
  function addFilename(error, options) {
    if (error && typeof error === 'object' && typeof error.stack === 'string') {
      const filename = typeof options === 'string' ? options : options?.filename;
      if (filename && !error.stack.startsWith(`${filename}:`)) {
        error.stack = `${filename}:1\n${error.stack}`;
      }
    }
  }
  function sourceOf(code) {
    if (typeof code === 'object' && code !== null && typeof code.code === 'string') return code.code;
    return String(code);
  }
  function runInThisContext(code, options) {
    try { return evaluateScript(mainGlobal, sourceOf(code), filenameOf(options)); }
    catch (error) { addFilename(error, options); throw error; }
  }
  function runInContext(code, context, options = {}) {
    const realm = context !== null && (typeof context === 'object' || typeof context === 'function')
      ? realms.get(context) : undefined;
    if (realm === undefined) {
      const error = new TypeError('The "contextifiedObject" argument must be a vm.Context.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    forwardSandbox(context, realm);
    realm.depth++;
    try { return evaluateScript(realm.global, sourceOf(code), filenameOf(options)); }
    catch (error) { addFilename(error, options); throw error; }
    finally {
      realm.depth--;
      reflectGlobals(context, realm);
    }
  }
  function runInNewContext(code, context = {}, options = {}) {
    return runInContext(code, createContext(context, options), options);
  }
  function compileFunction(code, params = [], options = {}) {
    if (!Array.isArray(params)) throw new TypeError('The "params" argument must be an array.');
    const fn = Function(...params.map(String), String(code));
    return fn;
  }
  const constants = Object.freeze({ USE_MAIN_CONTEXT_DEFAULT_LOADER: -1 });
  return {
    Script, createScript: (code, options) => new Script(code, options),
    createContext, isContext, runInThisContext, runInContext, runInNewContext,
    compileFunction, constants,
    measureMemory() { return Promise.reject(new Error('vm.measureMemory is unavailable')); },
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
