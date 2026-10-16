'use strict';
const get=require('internal/test/binding').internalBinding('errors').getErrorSourcePositions;
function show(label,e){const p=get(e);console.log(label,JSON.stringify(p),Object.getPrototypeOf(p)===null,Object.keys(p).join(','));}
function factory(){return new Error('x')}
show('fresh',factory());
Error.stackTraceLimit=0;show('zero',factory());
Error.stackTraceLimit=undefined;show('disabled',factory());
Error.stackTraceLimit=10;
function trim(){const e={};Error.captureStackTrace(e,trim);return e}
show('trimmed',trim());
const e=factory();Object.defineProperty(e,'stack',{value:'custom'});show('assigned',e);
const p=new Proxy(e,{get(){throw Error('trap')},getPrototypeOf(){throw Error('trap')}});show('proxy',p);
show('eval',eval('new Error("eval")'));
