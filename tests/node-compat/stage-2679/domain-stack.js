"use strict";

const assert = require("assert");
const domain = require("domain");

const outer = domain.create();
const inner = domain.create();
outer.enter();
inner.enter();
assert.deepStrictEqual(domain._stack, [outer, inner]);
outer.exit();
assert.deepStrictEqual(domain._stack, []);
