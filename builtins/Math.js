// Self-hosted Math builtins on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

// Save native implementations before overriding
var _nativeFloor = Math.floor;
var _nativeCeil = Math.ceil;
var _nativeRound = Math.round;
var _nativeTrunc = Math.trunc;
var _nativeSign = Math.sign;
var _nativePow = Math.pow;
var _nativeSqrt = Math.sqrt;

// Math.max (ES2025 §21.3.2.24)
Math.max = function MathMax() {
  var max = -Infinity;
  for (var i = 0; i < arguments.length; i++) {
    var n = Number(arguments[i]);
    if (n !== n) return n;
    if (n > max) max = n;
  }
  return max;
};

// Math.min (ES2025 §21.3.2.27)
Math.min = function MathMin() {
  var min = Infinity;
  for (var i = 0; i < arguments.length; i++) {
    var n = Number(arguments[i]);
    if (n !== n) return n;
    if (n < min) min = n;
  }
  return min;
};

// Math.abs (ES2025 §21.3.2.1)
Math.abs = function MathAbs(x) {
  var n = Number(x);
  return n < 0 ? -n : (n === 0 ? 0 : n);
};

// Math.floor (ES2025 §21.3.2.16)
Math.floor = function MathFloor(x) {
  return _nativeFloor(Number(x));
};

// Math.ceil (ES2025 §21.3.2.9)
Math.ceil = function MathCeil(x) {
  return _nativeCeil(Number(x));
};

// Math.round (ES2025 §21.3.2.30)
Math.round = function MathRound(x) {
  return _nativeRound(Number(x));
};

// Math.trunc (ES2025 §21.3.2.37)
Math.trunc = function MathTrunc(x) {
  return _nativeTrunc(Number(x));
};

// Math.sign (ES2025 §21.3.2.33)
Math.sign = function MathSign(x) {
  return _nativeSign(Number(x));
};

// Math.pow (ES2025 §21.3.2.29)
Math.pow = function MathPow(x, y) {
  return _nativePow(Number(x), Number(y));
};

// Math.sqrt (ES2025 §21.3.2.34)
Math.sqrt = function MathSqrt(x) {
  return _nativeSqrt(Number(x));
};
