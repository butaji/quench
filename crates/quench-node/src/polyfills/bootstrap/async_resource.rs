//! Small VM closure factory used by the Rust AsyncResource host object.

pub const JS: &str = quench_js_check::checked_js!(r#"
"use strict";
Object.defineProperty(globalThis, "__nodeAsyncResourceBind", {
  configurable: true,
  enumerable: false,
  value: (resource, callback, thisArg, hasThisArg) => {
    const holder = { resource, callback, thisArg, hasThisArg };
    const bound = function (...args) {
      let receiver = holder.hasThisArg ? holder.thisArg : this;
      if (!holder.hasThisArg) {
        if (receiver === globalThis) {
          receiver = undefined;
        } else {
          const tag = Object.prototype.toString.call(receiver);
          if (tag === "[object String]" || tag === "[object Number]" ||
              tag === "[object Boolean]") {
            receiver = receiver.valueOf();
          }
        }
      }
      if (holder.resource && typeof holder.resource.runInAsyncScope === "function") {
        return holder.resource.runInAsyncScope(holder.callback, receiver, ...args);
      }
      const previous = globalThis.__nodeCurrentAsyncResource;
      globalThis.__nodeCurrentAsyncResource = holder.resource;
      try { return holder.callback.apply(receiver, args); }
      finally { globalThis.__nodeCurrentAsyncResource = previous; }
    };
    Object.defineProperty(bound, "length", { value: holder.callback.length });
    return bound;
  }
});
"#);
