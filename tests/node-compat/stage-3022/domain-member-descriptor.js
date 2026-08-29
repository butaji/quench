"use strict";

const assert = require("assert");
const domain = require("domain").create();
const member = {};
const isEnumerable = Function.call.bind(Object.prototype.propertyIsEnumerable);

domain.add(member);
assert.strictEqual(member.domain, domain);
assert.strictEqual(isEnumerable(member, "domain"), false);
const previousMemberCount = domain.members.length;
domain.add(member);
assert.strictEqual(domain.members.length, previousMemberCount);
domain.remove(member);
assert.strictEqual(member.domain, undefined);
assert.strictEqual(isEnumerable(member, "domain"), false);
