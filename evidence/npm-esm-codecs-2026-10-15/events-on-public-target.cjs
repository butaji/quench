const assert=require('assert'),{on,EventEmitter}=require('events');
(async()=>{
const target=new EventTarget(),iterator=on(target,'value');
const pending=iterator.next(),event=new Event('value');target.dispatchEvent(event);
assert.strictEqual((await pending).value[0],event);await iterator.return();
const controller=new AbortController();controller.signal.addEventListener('abort',event=>event.stopImmediatePropagation());
const aborted=on(new EventEmitter(),'value',{signal:controller.signal});
const wait=aborted.next();controller.abort('reason');
await assert.rejects(wait,error=>error.code==='ABORT_ERR'&&error.cause==='reason');
console.log(JSON.stringify({complete:true}));
})().catch(error=>{console.error(error);process.exitCode=1});
