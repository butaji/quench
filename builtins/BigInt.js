var _bigIntToString = BigInt.prototype.__toString;
var _bigIntValueOf = BigInt.prototype.__valueOf;
var _bigIntAsIntN = BigInt.__asIntN;
var _bigIntAsUintN = BigInt.__asUintN;

BigInt.prototype.toString = function BigIntToString(radix) {
  return _bigIntToString.call(this, radix);
};

BigInt.prototype.valueOf = function BigIntValueOf() {
  return _bigIntValueOf.call(this);
};

BigInt.asIntN = function BigIntAsIntN(bits, bigint) {
  return _bigIntAsIntN.call(this, bits, bigint);
};

BigInt.asUintN = function BigIntAsUintN(bits, bigint) {
  return _bigIntAsUintN.call(this, bits, bigint);
};
