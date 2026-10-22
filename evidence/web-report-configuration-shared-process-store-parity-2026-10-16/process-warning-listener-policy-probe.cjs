'use strict';
console.log(JSON.stringify(['initial',process.listeners('warning').map(fn=>[fn.name,fn.length])]));
process.removeAllListeners('warning');
process.emitWarning('removed default listener');
process.nextTick(()=>console.log('finished'));
