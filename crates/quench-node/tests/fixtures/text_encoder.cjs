"use strict";

// Run under Node and Quench and compare the complete output, including error order.
const Encoder = TextEncoder;
const util = require('node:util');
const encoder = new Encoder();
const observations = [];
const observe = (name, run) => {
  try {
    observations.push([name, run()]);
  } catch (error) {
    observations.push([name, error.name, error.code || null, error.message]);
  }
};
const shape = object => Reflect.ownKeys(object).map(key => {
  const d = Object.getOwnPropertyDescriptor(object, key);
  return [String(key), d.writable, d.enumerable, d.configurable,
    typeof d.value === "function" ? [d.value.name, d.value.length] : d.value,
    d.get && [d.get.name, d.get.length], typeof d.set];
});
observe("constructor", () => shape(Encoder).filter(d => d[0] !== "prototype"));
observe("prototype-property", () => {
  const d = Object.getOwnPropertyDescriptor(Encoder, "prototype");
  return [d.writable, d.enumerable, d.configurable];
});
observe("prototype", () => shape(Encoder.prototype));
observe("instance", () => [encoder.encoding, Object.prototype.toString.call(encoder), Reflect.ownKeys(encoder)]);
observe("call", () => Encoder());
for (const input of [undefined, null, false, 123, "", "ASCII", "é水🦄", "\ud800", "\udfff", "\ud800a\udfff", "\ud800\ud800\udc00"]) {
  observe("encode " + String(input), () => Array.from(encoder.encode(input)));
}
observe("symbol", () => encoder.encode(Symbol("input")));
observe("coercion", () => {
  const events = [];
  const result = encoder.encode({
    [Symbol.toPrimitive](hint) { events.push(hint); return "🦄\udfff"; }
  });
  return [events, Array.from(result)];
});
observe("fresh-output", () => encoder.encode("a") !== encoder.encode("a"));
for (const input of ["aé水🦄z", "\ud800x\udfff", "🦄", ""]) {
  for (let capacity = 0; capacity < 13; capacity++) {
    observe("into " + input + " " + capacity, () => {
      const backing = new Uint8Array(capacity + 4).fill(255);
      const result = encoder.encodeInto(input, backing.subarray(2, 2 + capacity));
      return [result, Array.from(backing)];
    });
  }
}
for (const receiver of [{}, Object.create(encoder), Encoder.prototype, null, undefined, 1]) {
  observe("bad encode " + String(receiver), () => Encoder.prototype.encode.call(receiver, { toString() { throw new Error("coercion"); } }));
  observe("bad getter " + String(receiver), () => Object.getOwnPropertyDescriptor(Encoder.prototype, "encoding").get.call(receiver));
}
observe("argument-before-receiver", () => Encoder.prototype.encodeInto.call({}, 1, []));
observe("destination-before-receiver", () => Encoder.prototype.encodeInto.call({}, "a", []));
observe("receiver-after-arguments", () => Encoder.prototype.encodeInto.call({}, "a", new Uint8Array(1)));
observe("into-symbol", () => encoder.encodeInto(Symbol("input"), new Uint8Array(1)));
observe("into-clamped", () => encoder.encodeInto("a", new Uint8ClampedArray(1)));
observe("into-uint16", () => encoder.encodeInto("a", new Uint16Array(1)));
observe("coded-error-layout", () => {
  let first, second;
  try { encoder.encodeInto(1, []); } catch (error) { first = error; }
  try { encoder.encodeInto(2, []); } catch (error) { second = error; }
  return [first instanceof TypeError, first.constructor === TypeError, String(first),
    Object.getPrototypeOf(first) === Object.getPrototypeOf(second),
    Reflect.ownKeys(first).map(String),
    ["code", "message"].map(key => {
      const d = Object.getOwnPropertyDescriptor(first, key);
      return [key, d.value, d.writable, d.enumerable, d.configurable];
    }), shape(Object.getPrototypeOf(first))];
});
observe("detached", () => {
  const bytes = new Uint8Array(3);
  bytes.buffer.transfer();
  return encoder.encodeInto("a", bytes);
});
observe("shared", () => {
  const bytes = new Uint8Array(new SharedArrayBuffer(4));
  return [encoder.encodeInto("水x", bytes), Array.from(bytes)];
});
observe("proxy", () => {
  const proxy = new Proxy(encoder, { get() { throw new Error("proxy get"); }, getOwnPropertyDescriptor() { throw new Error("proxy descriptor"); } });
  return Encoder.prototype.encode.call(proxy, "a");
});
observe("revoked-proxy", () => {
  const { proxy, revoke } = Proxy.revocable(encoder, {});
  revoke();
  return Encoder.prototype.encode.call(proxy, "a");
});
observe("frozen-subclass", () => {
  class Custom extends Encoder {}
  const custom = Object.freeze(new Custom());
  return [custom instanceof Custom, custom instanceof Encoder, Array.from(custom.encode("é")), custom.encoding];
});
observe("custom-new-target", () => {
  function Other() {}
  const custom = Reflect.construct(Encoder, [], Other);
  return [Object.getPrototypeOf(custom) === Other.prototype, Array.from(Encoder.prototype.encode.call(custom, "é"))];
});
observe("negative-inspect", () => Encoder.prototype[Symbol.for("nodejs.util.inspect.custom")].call({}, -1) instanceof Object);
observe("global-replacement", () => {
  globalThis.TextEncoder = null;
  return [encoder.encoding, Array.from(encoder.encode("still works")), util.TextEncoder === Encoder];
});
console.log(observations.map(observation => JSON.stringify(observation)).join("\n"));
