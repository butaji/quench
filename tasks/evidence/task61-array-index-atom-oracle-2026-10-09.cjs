var ordinary = []; ordinary[599] = 17;
if (ordinary[599] !== 17 || !Object.prototype.hasOwnProperty.call(ordinary, '599') || ordinary.length !== 600) throw new Error('ordinary dense indexed write mismatch');

Object.defineProperty(Array.prototype, '507', { configurable: true, enumerable: true, writable: true, value: 12 });
var dataArray = []; dataArray[507] = 21;
if (dataArray[507] !== 21 || !Object.prototype.hasOwnProperty.call(dataArray, '507') || dataArray.length !== 508) throw new Error('inherited default data assignment mismatch');
delete Array.prototype[507];

var calls = 0, owner, seen;
Object.defineProperty(Array.prototype, '5', { configurable: true, set: function (value) { calls++; owner = this; seen = value; } });
var setterArray = [0]; setterArray[5] = 42;
if (calls !== 1 || owner !== setterArray || seen !== 42 || Object.prototype.hasOwnProperty.call(setterArray, '5') || setterArray.length !== 1) throw new Error('inherited indexed setter mismatch');
delete Array.prototype[5];

var trapCount = 0;
var proxyProto = new Proxy(Array.prototype, { set: function (target, key, value, receiver) { if (key === '509') trapCount++; return Reflect.set(target, key, value, receiver); } });
var proxyArray = []; Object.setPrototypeOf(proxyArray, proxyProto); proxyArray[509] = 33;
if (trapCount !== 1 || proxyArray[509] !== 33 || !Object.prototype.hasOwnProperty.call(proxyArray, '509') || proxyArray.length !== 510) throw new Error('proxy prototype indexed assignment mismatch');

var typedProto = new Uint8Array([11]);
var typedArrayReceiver = []; Object.setPrototypeOf(typedArrayReceiver, typedProto); typedArrayReceiver[0] = 99;
if (typedArrayReceiver[0] !== 99 || typedProto[0] !== 11 || !Object.prototype.hasOwnProperty.call(typedArrayReceiver, '0')) throw new Error('typed-array prototype indexed assignment mismatch');

var sloppy = []; Object.preventExtensions(sloppy); sloppy[503] = 1;
if (sloppy.length !== 0 || Object.prototype.hasOwnProperty.call(sloppy, '503')) throw new Error('sloppy non-extensible write mismatch');
var strictArray = []; Object.preventExtensions(strictArray);
var threw = false;
(function () { 'use strict'; try { strictArray[503] = 1; } catch (error) { threw = error instanceof TypeError; } })();
if (!threw || strictArray.length !== 0 || Object.prototype.hasOwnProperty.call(strictArray, '503')) throw new Error('strict non-extensible write mismatch');

console.log('indexed-write oracle: PASS');

var deletionArray = [];
deletionArray[1023] = 8;
var deletionLength = deletionArray.length;
if (delete deletionArray[1023] !== true || deletionArray.length !== deletionLength || Object.prototype.hasOwnProperty.call(deletionArray, '1023') || 1023 in deletionArray) throw new Error('indexed deletion mismatch');
deletionArray[1023] = 19;
if (deletionArray[1023] !== 19 || deletionArray.length !== deletionLength) throw new Error('indexed re-add mismatch');

var accessorReads = 0, accessorWrites = 0, accessorValue = 5;
var accessorArray = [];
Object.defineProperty(accessorArray, '514', { configurable: true, get: function () { accessorReads++; return accessorValue; }, set: function (value) { accessorWrites++; accessorValue = value; } });
if (accessorArray[514] !== 5 || accessorReads !== 1) throw new Error('own indexed getter mismatch');
accessorArray[514] = 23;
if (accessorValue !== 23 || accessorWrites !== 1 || accessorArray.length !== 515) throw new Error('own indexed setter mismatch');
delete accessorArray[514];
accessorArray[514] = 29;
if (accessorArray[514] !== 29 || accessorArray.length !== 515) throw new Error('accessor replacement after delete mismatch');

var holeArray = [];
holeArray.length = 1024;
holeArray[1023] = undefined;
if (!(1023 in holeArray) || !Object.prototype.hasOwnProperty.call(holeArray, '1023')) throw new Error('explicit undefined indexed property mismatch');
delete holeArray[1023];
if (1023 in holeArray || Object.prototype.hasOwnProperty.call(holeArray, '1023') || holeArray.length !== 1024) throw new Error('hole creation mismatch');

console.log('array-index oracle extensions: PASS');
