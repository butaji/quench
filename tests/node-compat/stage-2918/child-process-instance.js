'use strict'; const assert=require('assert'); const cp=require('child_process'); const c=cp.spawn(process.execPath,['-p','42']); assert(c instanceof cp.ChildProcess);
