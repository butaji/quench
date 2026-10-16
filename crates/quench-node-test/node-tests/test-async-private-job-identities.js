const ah = require('node:async_hooks');
const als = new ah.AsyncLocalStorage();
const rows=[];
const hook=ah.createHook({init(id,type,trigger){if(type==='PROMISE'||type==='Timeout') rows.push(['init',id,type,trigger]);},before(id){rows.push(['before',id]);},after(id){rows.push(['after',id]);},promiseResolve(id){rows.push(['resolve',id]);}}).enable();
const mark=(s)=>rows.push(['mark',s,ah.executionAsyncId(),ah.triggerAsyncId(),als.getStore()]);
als.run('root',()=>{
 mark('sync');
 (async()=>{await 1;mark('await1');await undefined;mark('await2');})();
 Promise.resolve().then(()=>{mark('then1');als.enterWith('chain');return 3;}).then(()=>mark('then2'));
 setTimeout(()=>{mark('timer1');setTimeout(()=>{mark('timer2');hook.disable();process.stdout.write(JSON.stringify(rows)+'\n');},0)},0);
});
