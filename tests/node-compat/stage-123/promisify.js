const util = require('util');
if (typeof util.promisify !== 'function') throw new Error('promisify missing');
const wrapped = util.promisify((value, callback) => callback(null, value + 1));
wrapped(41).then((value) => { if (value !== 42) throw new Error('promisify mismatch'); });
