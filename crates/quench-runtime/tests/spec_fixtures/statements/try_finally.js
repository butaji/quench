// spec: ECMA-262 sec-try-statement-runtime-semantics
// expect: value: "finally"
// tags: statements, try-finally

let result = "try";
try {
  // no throw
} finally {
  result = "finally";
}
result;
