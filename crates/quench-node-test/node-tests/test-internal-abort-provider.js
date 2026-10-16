'use strict';
const {abortSignal}=require('internal/abort_controller');
const marker={},other={},controller=new AbortController(),signal=controller.signal,events=[];
signal.addEventListener('abort',function(e){events.push([this===signal,e.type,e.isTrusted,signal.aborted,signal.reason===marker])});
const dependent=AbortSignal.any([signal]);dependent.addEventListener('abort',()=>events.push(['dependent',dependent.reason===marker]));
console.log('operation',abortSignal.name,abortSignal.length,abortSignal(signal,marker),abortSignal(signal,other));
console.log('state',signal.aborted,signal.reason===marker,dependent.aborted,JSON.stringify(events));
const undefinedReason=new AbortController().signal;abortSignal(undefinedReason,undefined);console.log('undefined',undefinedReason.aborted,undefinedReason.reason===undefined);
try{signal.throwIfAborted()}catch(e){console.log('reason identity',e===marker)}
