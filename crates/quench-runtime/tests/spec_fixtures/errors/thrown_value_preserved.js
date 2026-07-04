// spec: ecma-262 sec-try-statement-runtime-semantics
// expect: value: "preserved"
// tags: errors, try-catch, exceptions

try {
  throw new Error("preserved");
} catch (e) {
  e.message;
}
