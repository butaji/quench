const log = value => console.log(String(value));

const regexp = /a(b)?/g;
log(JSON.stringify([regexp.source, regexp.flags, regexp.exec("za"), regexp.lastIndex]));
regexp.compile("x+", "iy");
log(JSON.stringify([regexp.source, regexp.flags, regexp.test("XX"), regexp.lastIndex]));
const indexed = /(?<x>a)(b)/d.exec("ab");
log(JSON.stringify([indexed.indices[0], indexed.indices.groups.x]));
log(JSON.stringify(new RegExp("\\u{1F600}", "u").source));

function withScope() {
  const outer = 4;
  const scope = { outer: 7, value: 9 };
  with (scope) {
    return () => [outer, value, typeof missing];
  }
}
log(JSON.stringify(withScope()()));

function directEval() {
  let value = 12;
  eval("value = value + 3");
  return value;
}
log(directEval());

(async () => {
  const values = await Array.fromAsync(
    { async *[Symbol.asyncIterator]() { yield 2; yield 3; } },
    async value => value * 5,
  );
  log(JSON.stringify(values));
})().catch(error => {
  log(error.name + ":" + error.message);
  process.exitCode = 1;
});
