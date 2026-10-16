'use strict';
function check(make) {
  var value = make();
  var generator = Object.getPrototypeOf(Object.getPrototypeOf(value));
  var asyncIterator = Object.getPrototypeOf(generator);
  console.log(typeof value[Symbol.iterator]);
  console.log(typeof value[Symbol.asyncIterator]);
  console.log(Object.getPrototypeOf(asyncIterator) === Object.prototype);
  console.log(Object.hasOwn(generator, Symbol.asyncIterator));
  console.log(typeof value.map, typeof value.take, typeof value[Symbol.dispose]);
  console.log(value[Symbol.asyncIterator]() === value);
  console.log(Object.keys(Object.getOwnPropertyDescriptor(asyncIterator, Symbol.asyncIterator)).join(','));
}
check(function () { return (async function* () { yield 1; })(); });
check(function () { return new (Object.getPrototypeOf(async function* () {}).constructor)('yield 1')(); });

const vm=require('node:vm');
const source='('+check.toString()+')(()=>(async function*(){yield 1})())';
vm.runInNewContext(source,{console});
