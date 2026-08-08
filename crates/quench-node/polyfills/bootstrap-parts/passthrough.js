// The stream polyfill is initialized before this compatibility hook. Keep
// PassThrough on the canonical implementation so it shares EventEmitter,
// backpressure, and finish semantics with Readable/Writable.
const __quenchStreamRequire = globalThis.require;
globalThis.require = (specifier) => __quenchStreamRequire(specifier);
