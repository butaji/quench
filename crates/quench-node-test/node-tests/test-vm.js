// Node compat: vm + string_decoder shape.
const vm = require('node:vm');
const sd = require('node:string_decoder');
if (typeof vm !== 'object') throw new Error('vm: ' + typeof vm);
if (typeof sd.StringDecoder !== 'function') throw new Error('StringDecoder: ' + typeof sd.StringDecoder);
console.log('vm+sd: ok');
const f = vm.compileFunction('return a + b', ['a', 'b']);
if (f(2, 3) !== 5) throw new Error('compileFunction');
const s = new vm.Script('x + 1');
if (s.runInNewContext({x: 4}) !== 5) throw new Error('Script');
const ctx = vm.createContext({x: 8});
if (!vm.isContext(ctx) || vm.runInContext('x + 2', ctx) !== 10) throw new Error('context');
