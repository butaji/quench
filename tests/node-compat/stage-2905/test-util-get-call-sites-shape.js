'use strict';

const assert = require('assert');
const { getCallSites } = require('util');

const [callSite] = getCallSites(1);
assert.strictEqual(typeof callSite.scriptName, 'string');
assert.strictEqual(typeof callSite.lineNumber, 'number');
assert.strictEqual(typeof callSite.columnNumber, 'number');
