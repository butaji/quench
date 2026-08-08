// Self-hosted Number static methods (ES §20.1.2). Pure-spec, no coercion.
// Named function expressions no longer leak their name to the enclosing scope,
// so `function isNaN` here does not shadow the global `isNaN`.

// Number.isNaN(number) — §20.1.2.4: true iff `number` is a Number and NaN.
Number.isNaN = function isNaN(number) {
  return typeof number === 'number' && number !== number;
};

// Number.isFinite(number) — §20.1.2.2: true iff `number` is a finite Number.
Number.isFinite = function isFinite(number) {
  return (
    typeof number === 'number' &&
    number === number &&
    number !== Infinity &&
    number !== -Infinity
  );
};

// Number.isInteger(number) — §20.1.2.3: true iff `number` is an integer Number.
Number.isInteger = function isInteger(number) {
  return Number.isFinite(number) && number % 1 === 0;
};

// Number.isSafeInteger(number) — §20.1.2.5: an integer within ±2^53−1.
Number.isSafeInteger = function isSafeInteger(number) {
  return Number.isInteger(number) && Math.abs(number) <= 9007199254740991;
};