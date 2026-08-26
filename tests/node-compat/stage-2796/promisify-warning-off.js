'use strict';
const common = require('../../node/test/common');
const { promisify } = require('util');
const warningHandler = common.mustNotCall();
process.on('warning', warningHandler);
function foo() {}
promisify(foo);
process.off('warning', warningHandler);
common.expectWarning('DeprecationWarning', 'Calling promisify on a function that returns a Promise is likely a mistake.', 'DEP0174');
promisify(async (callback) => callback())();
