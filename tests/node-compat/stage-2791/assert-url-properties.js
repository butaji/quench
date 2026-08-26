const assert = require("assert");

const different = [new URL("http://foo"), new URL("http://foo")];
different[0].tag = 1;
different[1].tag = 2;
assert.throws(() => assert.deepStrictEqual(...different), {
  code: "ERR_ASSERTION",
});

const same = [new URL("http://foo"), new URL("http://foo")];
same[0].tag = 1;
same[1].tag = 1;
assert.deepStrictEqual(...same);
