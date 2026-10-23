const origMap = Array.prototype.map;
Array.prototype.map = function () { throw new Error('mutated'); };
Object.defineProperty(Object.prototype, 'evil', { get() { throw new Error('evil'); }, configurable: true });
for (let i = 0; i < 50; i++) { globalThis.gc(); const junk = []; for (let j = 0; j < 2e4; j++) junk.push({ j }); }
const util = require('util'); const path = require('path'); const events = require('events');
console.log(util.inspect({ a: [1, 2] }), path.join('a', 'b'), typeof events.EventEmitter, Array.prototype.map === origMap);
