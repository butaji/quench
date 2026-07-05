// spec: ECMA-262 sec-try-statement-runtime-semantics
// expect: value: "caught"
// tags: statements, try-catch

let result = "not caught";
try {
  throw new Error("test");
} catch (e) {
  result = "caught";
}
result;
