//! Shared-VM implementation of the selected diagnostics-channel behavior.

use crate::host::NodeHost;
use rqj::{NativeContext, RootId, RootedError};

const API: &str = quench_js_check::checked_js!(
    r#"(() => {
  const channels = new Map();
  const badSubscriber = () => Object.assign(
    new TypeError('The "subscriber" argument must be of type function'),
    { code: 'ERR_INVALID_ARG_TYPE' },
  );

  class Channel {
    constructor(name) {
      this.name = String(name);
      this._subscribers = [];
      this._storeBindings = [];
    }

    get hasSubscribers() { return this._subscribers.length !== 0; }

    subscribe(subscriber) {
      if (typeof subscriber !== 'function') throw badSubscriber();
      if (!this._subscribers.includes(subscriber)) this._subscribers.push(subscriber);
    }

    unsubscribe(subscriber) {
      const index = this._subscribers.indexOf(subscriber);
      if (index < 0) return false;
      this._subscribers.splice(index, 1);
      return true;
    }

    publish(message) {
      for (const subscriber of this._subscribers.slice()) {
        Reflect.apply(subscriber, undefined, [message, this.name]);
      }
    }

    bindStore(store, transform = (message) => message) {
      if (typeof transform !== 'function') throw badSubscriber();
      this._storeBindings.push({ store, transform });
    }
  }

  const channel = (name) => {
    const key = String(name);
    let value = channels.get(key);
    if (value === undefined) {
      value = new Channel(key);
      channels.set(key, value);
    }
    return value;
  };

  const hasSubscribers = (name) => channel(name).hasSubscribers;
  const subscribe = (name, listener) => channel(name).subscribe(listener);
  const unsubscribe = (name, listener) => channel(name).unsubscribe(listener);

  class TracingChannel {
    constructor(name) {
      this.name = String(name);
      this.start = channel(`${this.name}:start`);
      this.end = channel(`${this.name}:end`);
      this.asyncStart = channel(`${this.name}:asyncStart`);
      this.asyncEnd = channel(`${this.name}:asyncEnd`);
      this.error = channel(`${this.name}:error`);
    }

    traceSync(callback, ...args) {
      this.start.publish(args[0]);
      let invoke = () => Reflect.apply(callback, undefined, args);
      for (const binding of this.start._storeBindings) {
        const next = invoke;
        invoke = () => binding.store.run(binding.transform(args[0]), next);
      }
      try {
        return invoke();
      } catch (error) {
        this.error.publish(error);
        throw error;
      } finally {
        this.end.publish(args[0]);
      }
    }
  }

  const tracingChannel = (name) => new TracingChannel(name);
  return { Channel, channel, hasSubscribers, subscribe, unsubscribe, tracingChannel };
})()"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    context.evaluate_script_rooted(API, "node:diagnostics_channel/shared-api.js")
}
