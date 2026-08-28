'use strict';
const common=require('../../node/test/common'); const assert=require('assert'); const cp=require('child_process'); const {promisify}=require('util');
const exec=promisify(cp.exec);
const promise=exec(...common.escapePOSIXShell`"${process.execPath}" -p 42`);
assert(promise.child instanceof cp.ChildProcess);
promise.then(common.mustCall((obj)=>assert.deepStrictEqual(obj,{stdout:'42\n',stderr:''})));
