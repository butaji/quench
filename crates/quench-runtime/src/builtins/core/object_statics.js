// Self-hosted Object static methods over the `__ops__` bridge.

// Object.is(a, b) — §20.1.2.10: SameValue (NaN equals NaN, +0 !== -0).
Object.is = function is(a, b) {
  return __ops__.sameValue(a, b);
};

// Object.isExtensible(o) — §19.1.2.11: IsExtensible.
Object.isExtensible = function isExtensible(o) {
  return __ops__.isExtensible(o);
};

// Object.getPrototypeOf(o) — §19.1.2.9: ToObject(o) then [[GetPrototypeOf]].
Object.getPrototypeOf = function getPrototypeOf(o) {
  return __ops__.getPrototypeOf(__ops__.toObject(o));
};

// Object.setPrototypeOf(o, proto) — §19.1.2.18: [[SetPrototypeOf]].
Object.setPrototypeOf = function setPrototypeOf(o, proto) {
  return __ops__.setPrototypeOf(o, proto);
};