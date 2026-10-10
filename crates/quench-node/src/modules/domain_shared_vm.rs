//! Compatibility implementation of Node's deprecated `domain` module.
//!
//! Domains provide explicit error routing for callbacks. This implementation
//! covers the public APIs used by legacy libraries and the Node regression
//! fixtures without coupling the domain stack to the host scheduler.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const DOMAIN_MODULE: &str = quench_js_check::checked_js!(r#"(EventEmitter, processObject) => {
  const stack = [];

  const report = (domain, error) => {
    if (error && (typeof error === 'object' || typeof error === 'function')) {
      if (error.domain === undefined) {
        Object.defineProperty(error, 'domain', { value: domain, writable: true, configurable: true, enumerable: false });
      }
      if (error.domainThrown === undefined) {
        Object.defineProperty(error, 'domainThrown', { value: true, writable: true, configurable: true, enumerable: false });
      }
    }
    if (!domain.emit('error', error)) throw error;
  };

  class Domain extends EventEmitter {
    constructor() {
      super();
      this.members = [];
      this.disposed = false;
    }

    enter() {
      if (this.disposed) return this;
      stack.push(this);
      return this;
    }

    exit() {
      const index = stack.lastIndexOf(this);
      if (index !== -1) stack.splice(index);
      return this;
    }

    run(callback, ...args) {
      if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function');
      this.enter();
      try {
        return Reflect.apply(callback, this, args);
      } catch (error) {
        report(this, error);
      } finally {
        this.exit();
      }
    }

    bind(callback) {
      if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function');
      const domain = this;
      function bound(...args) {
        if (domain.disposed) return Reflect.apply(callback, this, args);
        domain.enter();
        try {
          return Reflect.apply(callback, this, args);
        } catch (error) {
          report(domain, error);
        } finally {
          domain.exit();
        }
      }
      bound.domain = domain;
      return bound;
    }

    intercept(callback) {
      if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function');
      const domain = this;
      return domain.bind(function(error, ...args) {
        if (error) {
          if (error && (typeof error === 'object' || typeof error === 'function')) {
            Object.defineProperty(error, 'domainBound', { value: callback, writable: true, configurable: true, enumerable: false });
            Object.defineProperty(error, 'domainThrown', { value: false, writable: true, configurable: true, enumerable: false });
          }
          return report(domain, error);
        }
        return Reflect.apply(callback, this, args);
      });
    }

    add(emitter) {
      if (emitter == null || (typeof emitter !== 'object' && typeof emitter !== 'function')) return this;
      if (!this.members.includes(emitter)) this.members.push(emitter);
      Object.defineProperty(emitter, 'domain', { value: this, writable: true, configurable: true, enumerable: false });
      return this;
    }

    remove(emitter) {
      const index = this.members.indexOf(emitter);
      if (index !== -1) this.members.splice(index, 1);
      if (emitter?.domain === this) Object.defineProperty(emitter, 'domain', { value: undefined, writable: true, configurable: true, enumerable: false });
      return this;
    }

    dispose() {
      this.exit();
      for (const member of this.members.slice()) this.remove(member);
      this.disposed = true;
      return this;
    }
  }

  const currentDomain = () => stack[stack.length - 1] || null;
  Object.defineProperty(processObject, 'domain', {
    configurable: true,
    enumerable: true,
    get: currentDomain,
  });
  Object.defineProperty(globalThis, '__quench_active_domain', {
    configurable: true,
    get: currentDomain,
  });

  const bindScheduledCallback = (target, name) => {
    const original = target?.[name];
    if (typeof original !== 'function') return;
    target[name] = function(callback, ...args) {
      const current = stack[stack.length - 1];
      if (current && typeof callback === 'function') callback = current.bind(callback);
      return Reflect.apply(original, this, [callback, ...args]);
    };
  };
  bindScheduledCallback(processObject, 'nextTick');
  for (const name of ['setTimeout', 'setInterval', 'setImmediate']) bindScheduledCallback(globalThis, name);

  function create() { return new Domain(); }
  const module = { Domain, create, createDomain: create, _stack: stack };
  Object.defineProperty(module, 'active', { enumerable: true, get: currentDomain });
  return module;
}"#);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(DOMAIN_MODULE, "node:domain/shared.js")?;
    let events = crate::modules::events_shared_vm::module(context)?;
    let emitter_key = context.string_rooted("EventEmitter");
    let emitter = context.get_property_rooted(events, emitter_key)?;
    let global = context.global_root()?;
    let process_key = context.string_rooted("process");
    let process = context.get_property_rooted(global, process_key)?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[emitter, process])
}
