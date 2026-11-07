// Random BigInt operations whose printed results must match Node's: magnitudes from zero
// to past the inline digits, boundary digit patterns, both signs; every operator, the
// comparisons with Number and String, asIntN/asUintN, toString radices and the errors.
"use strict";

let state = 0x2545f491;
function next() {
  state ^= state << 13;
  state ^= state >>> 17;
  state ^= state << 5;
  return state >>> 0;
}

const DIGIT_PATTERNS = ["ffffffffffffffff", "8000000000000000", "0000000000000000", "0000000000000001"];
function digit() {
  const choice = next() % 8;
  if (choice < DIGIT_PATTERNS.length) return DIGIT_PATTERNS[choice];
  return next().toString(16).padStart(8, "0") + next().toString(16).padStart(8, "0");
}

function integer(maxDigits) {
  const length = next() % (maxDigits + 1);
  let hex = "0";
  for (let i = 0; i < length; i++) hex += digit();
  const value = BigInt("0x" + hex);
  return next() % 2 ? -value : value;
}

function attempt(operation) {
  try {
    return String(operation());
  } catch (error) {
    return `${error.constructor.name}: ${error.message}`;
  }
}

const P = 2n ** 255n - 19n;
const lines = [];
for (let round = 0; round < 3000; round++) {
  const maxDigits = round % 10 === 0 ? 20 : 9;
  const a = integer(maxDigits);
  const b = integer(maxDigits);
  const count = BigInt(next() % 300) - 150n;
  const small = BigInt(next() % 12);
  let increment = a;
  let decrement = a;
  increment++;
  decrement--;
  lines.push([
    a + b, a - b, a * b,
    attempt(() => a / b), attempt(() => a % b),
    attempt(() => a ** small),
    attempt(() => a << count), attempt(() => a >> count),
    a & b, a | b, a ^ b, ~a, -a, increment, decrement,
    a < b, a <= b, a > b, a >= b, a == b, a === b, a === a * 1n,
    ((a % P) + P) % P,
    BigInt.asIntN(Number(small) * 23, a), BigInt.asUintN(Number(small) * 23, a),
    a.toString(2 + (next() % 35)), Number(a),
    a < Number(b), a == Number(a), a == String(a), a < String(b),
  ].join(" "));
}

const errors = [
  () => 1n / 0n,
  () => 1n % 0n,
  () => 1n + 1,
  () => 1 * 1n,
  () => 1n >>> 0n,
  () => +1n,
  () => 1n << (1n << 40n),
  () => 2n ** (1n << 40n),
  () => (1n).toString(1),
  () => (1n).toString(37),
  () => BigInt(1.5),
  () => BigInt("0x"),
  () => BigInt("12n"),
];
for (const error of errors) lines.push(attempt(error));
lines.push(0n === -0n, -0n, (-1n) ** (1n << 40n), 0n ** (1n << 40n), (-5n) >> (1n << 40n));
console.log(lines.join("\n"));
