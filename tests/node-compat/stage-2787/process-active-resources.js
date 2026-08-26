'use strict';
const assert = require('assert');

const timer = setTimeout(() => {}, 1000);
assert.deepStrictEqual(process.getActiveResourcesInfo(), ['Timeout']);
clearTimeout(timer);
assert.deepStrictEqual(process.getActiveResourcesInfo(), []);
