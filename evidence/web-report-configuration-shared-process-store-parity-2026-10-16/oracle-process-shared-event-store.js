'use strict';
const {EventEmitter}=require('events');
const p=process;
const type='parity-process-events';
const symbol=Symbol('parity-process-events');
const out=[];
function first(value){out.push(['first',value,this===p]);}
function second(value){out.push(['second',value,this===p]);}
function onlyOnce(value){out.push(['once',value,this===p]);}
out.push(['methods',p.on===EventEmitter.prototype.on,p.emit===EventEmitter.prototype.emit,Object.hasOwn(p,'on')]);
p.on(type,first);EventEmitter.prototype.on.call(p,type,second);p.once(type,onlyOnce);p.prependListener(type,second);p.on(symbol,first);
out.push(['counts',p.listenerCount(type),EventEmitter.prototype.listenerCount.call(p,type),p.listenerCount(type,second)]);
out.push(['store',Array.isArray(p._events[type]),p._events[type].length]);
out.push(['listeners',p.listeners(type).map(fn=>fn.name)]);
out.push(['raw',p.rawListeners(type).map(fn=>[fn.name,fn.length,Object.hasOwn(fn,'prototype'),fn.listener===onlyOnce])]);
out.push(['names',p.eventNames().filter(name=>name===type||name===symbol).map(name=>typeof name==='symbol'?'symbol':name)]);
p.emit(type,1);p.emit(type,2);p.removeListener(type,second);out.push(['removed',p.listenerCount(type),p.listenerCount(type,second)]);
p._events[type]=first;out.push(['replaced',p.listenerCount(type)]);EventEmitter.prototype.emit.call(p,type,3);
p.removeAllListeners(type);p.removeAllListeners(symbol);out.push(['cleared',p.listenerCount(type),p.listenerCount(symbol)]);
console.log(JSON.stringify(out));
