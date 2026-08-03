// Self-hosted Math builtins on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

// Save native implementations before overriding
var _nativeFloor = Math.__floor;
var _nativeCeil = Math.__ceil;
var _nativeRound = Math.__round;
var _nativeTrunc = Math.__trunc;
var _nativeSign = Math.__sign;
var _nativePow = Math.__pow;
var _nativeSqrt = Math.__sqrt;

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

Math.sin = function(x) { return Math.__sin(Number(x)); };
Math.cos = function(x) { return Math.__cos(Number(x)); };
Math.tan = function(x) { return Math.__tan(Number(x)); };
Math.asin = function(x) { return Math.__asin(Number(x)); };
Math.acos = function(x) { return Math.__acos(Number(x)); };
Math.atan = function(x) { return Math.__atan(Number(x)); };
Math.log = function(x) { return Math.__log(Number(x)); };
Math.log10 = function(x) { return Math.__log10(Number(x)); };
Math.log2 = function(x) { return Math.__log2(Number(x)); };
Math.exp = function(x) { return Math.__exp(Number(x)); };
Math.log1p = function(x) { return Math.__log1p(Number(x)); };
Math.cbrt = function(x) { return Math.__cbrt(Number(x)); };
Math.expm1 = function(x) { return Math.__expm1(Number(x)); };
Math.cosh = function(x) { return Math.__cosh(Number(x)); };
Math.sinh = function(x) { return Math.__sinh(Number(x)); };
Math.tanh = function(x) { return Math.__tanh(Number(x)); };
Math.acosh = function(x) { return Math.__acosh(Number(x)); };
Math.asinh = function(x) { return Math.__asinh(Number(x)); };
Math.atanh = function(x) { return Math.__atanh(Number(x)); };
Math.atan2 = function(y, x) { return Math.__atan2(Number(y), Number(x)); };
Math.imul = function(x, y) { return Math.__imul(Number(x), Number(y)); };
Math.fround = function(x) { return Math.__fround(Number(x)); };
Math.clz32 = function(x) { return Math.__clz32(Number(x)); };
Math.hypot = function() { return Math.__hypot.apply(Math, arguments); };
Math.random = function() { return Math.__random(); };
