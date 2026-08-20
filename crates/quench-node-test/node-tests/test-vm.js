// Node compat: vm + string_decoder shape.
const vm = require('node:vm');
const sd = require('node:string_decoder');
if (typeof vm !== 'object') throw new Error('vm: ' + typeof vm);
if (typeof sd.StringDecoder !== 'function') throw new Error('StringDecoder: ' + typeof sd.StringDecoder);
console.log('vm+sd: ok');
