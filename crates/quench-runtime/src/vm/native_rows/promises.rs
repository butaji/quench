//! The rows of `Promise.prototype` and the `Promise` constructor's statics.

use super::*;

natives! {
    // `Promise.prototype`, in installation order (`[Symbol.toStringTag]` is symbol-keyed).
    (PromisePrototype, "then", PromiseThen, 2),
    (PromisePrototype, "catch", PromiseCatch, 1),
    (PromisePrototype, "finally", PromiseFinally, 1),
    // `Promise`'s statics, in installation order.
    (PromiseConstructor, "resolve", PromiseResolve, 1),
    (PromiseConstructor, "reject", PromiseReject, 1),
    (PromiseConstructor, "withResolvers", PromiseWithResolvers, 0),
    (PromiseConstructor, "all", PromiseAll, 1),
    (PromiseConstructor, "allKeyed", PromiseAllKeyed, 1),
    (PromiseConstructor, "race", PromiseRace, 1),
    (PromiseConstructor, "allSettled", PromiseAllSettled, 1),
    (PromiseConstructor, "allSettledKeyed", PromiseAllSettledKeyed, 1),
    (PromiseConstructor, "any", PromiseAny, 1),
    (PromiseConstructor, "try", PromiseTry, 1),
}
