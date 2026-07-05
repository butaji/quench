// spec: ECMA-262 sec-switch-statement-runtime-semantics
// expect: value: "two"
// tags: statements, switch

const x = 2;
let result = "";
switch (x) {
  case 1:
    result = "one";
    break;
  case 2:
    result = "two";
    break;
  default:
    result = "other";
}
result;
