//! Synchronous AsyncLocalStorage behavior for the shared VM.

use crate::host::NodeHost;
use rqj::{NativeContext, RootId, RootedError};

const MODULE: &str = quench_js_check::checked_js!(
    r#"(() => {
  class AsyncLocalStorage {
    constructor() {
      this._store = undefined;
    }
    run(store, callback, ...args) {
      const previous = this._store;
      this._store = store;
      try {
        return callback(...args);
      } finally {
        this._store = previous;
      }
    }
    getStore() {
      return this._store;
    }
    enterWith(store) {
      this._store = store;
    }
    exit(callback, ...args) {
      const previous = this._store;
      this._store = undefined;
      try {
        return callback(...args);
      } finally {
        this._store = previous;
      }
    }
    disable() {
      this._store = undefined;
    }
  }
  return { AsyncLocalStorage };
})()"#
);

pub(crate) fn module(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    context.evaluate_script_rooted(MODULE, "node:async_hooks/shared.js")
}
