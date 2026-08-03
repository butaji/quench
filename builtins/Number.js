// Self-hosted Number prototype methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

// Save native implementations
var _nativeToFixed = Number.prototype.toFixed;
var _nativeToExponential = Number.prototype.toExponential;
var _nativeToPrecision = Number.prototype.toPrecision;

Number.isNaN = function NumberIsNaN(value) {
  return typeof value === 'number' && value !== value;
};

Number.isFinite = function NumberIsFinite(value) {
  return typeof value === 'number' && value !== Infinity && value !== -Infinity && value === value;
};

Number.isInteger = function NumberIsInteger(value) {
  return Number.isFinite(value) && Math.floor(value) === value;
};

Number.isSafeInteger = function NumberIsSafeInteger(value) {
  return Number.isInteger(value) && Math.abs(value) <= 9007199254740991;
};

// Number.prototype.toFixed (ES2025 §21.1.3.5)
Number.prototype.toFixed = function NumberToFixed(digits) {
  if (this === null || this === undefined) throw ThrowTypeError("Number.prototype.toFixed called on null or undefined");
  return _nativeToFixed.call(this, digits);
};

// Number.prototype.toExponential (ES2025 §21.1.3.4)
Number.prototype.toExponential = function NumberToExponential(digits) {
  if (this === null || this === undefined) throw ThrowTypeError("Number.prototype.toExponential called on null or undefined");
  return _nativeToExponential.call(this, digits);
};

// Number.prototype.toPrecision (ES2025 §21.1.3.6)
Number.prototype.toPrecision = function NumberToPrecision(precision) {
  if (this === null || this === undefined) throw ThrowTypeError("Number.prototype.toPrecision called on null or undefined");
  return _nativeToPrecision.call(this, precision);
};
