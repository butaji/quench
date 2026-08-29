'use strict';
const common=require('../../node/test/common'); const assert=require('assert'); const cp=require('child_process'); const {promisify}=require('util');
promisify(cp.exec)('doesntexist').catch(common.mustCall((err)=>assert(err.message.includes('doesntexist'))));
promisify(cp.execFile)('doesntexist',['-p','42']).catch(common.mustCall((err)=>assert(err.message.includes('doesntexist'))));
