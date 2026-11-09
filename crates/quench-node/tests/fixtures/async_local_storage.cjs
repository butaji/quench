'use strict';
const { AsyncLocalStorage: ALS, AsyncResource, createHook, executionAsyncId, triggerAsyncId }=require('node:async_hooks');
const out=[];
function observe(name,fn){try{out.push([name,fn()]);}catch(e){out.push([name,e.name,e.code||null,e.message]);}}
const shape=o=>Reflect.ownKeys(o).map(k=>{const d=Object.getOwnPropertyDescriptor(o,k);return[String(k),d.writable,d.enumerable,d.configurable,typeof d.value==='function'?[d.value.name,d.value.length]:undefined,d.get&&[d.get.name,d.get.length]];});
async function main(){
  observe('class metadata',()=>[ALS.name,ALS.length,AsyncResource.name,AsyncResource.length,shape(ALS.prototype)]);
  observe('instance layout',()=>Reflect.ownKeys(new ALS()).map(String));
  observe('constructor defaults',()=>[new ALS().name,new ALS({defaultValue:42}).getStore(),new ALS({name:0}).name]);
  observe('constructor getter order',()=>{const events=[];let n=0;const a=new ALS({get defaultValue(){events.push('default');return 7;},get name(){events.push('name');return ++n;}});return[events,a.name,a.getStore()];});
  for(const options of [null,1,false,[]])observe('invalid options '+String(options),()=>new ALS(options));
  observe('requires new',()=>ALS());
  const a=new ALS({defaultValue:'default'}),b=new ALS({defaultValue:'other'});
  observe('nested run',()=>[a.run('outer',()=>[a.getStore(),a.run('inner',()=>a.getStore()),a.getStore(),b.getStore()]),a.getStore()]);
  observe('run receiver',()=>a.run('receiver',function(x){return[this===null,x,a.getStore()];},6));
  observe('explicit undefined',()=>[a.run(undefined,()=>a.getStore()),a.getStore()]);
  observe('run restores exceptions',()=>{let same=false;const reason={reason:1};try{a.run('throw',()=>{throw reason;});}catch(e){same=e===reason;}return[same,a.getStore()];});
  observe('Object.is short circuit',()=>{const a=new ALS({defaultValue:NaN});let calls=0;a.enterWith=()=>{calls++;};a.run(NaN,()=>{});return calls;});
  observe('public defaultValue does not replace private default',()=>{const a=new ALS({defaultValue:4});a.defaultValue=9;return a.getStore();});
  observe('exit value',()=>a.run('outer',()=>[a.exit(function(x){return[this===null,x,a.getStore()];},8),a.getStore()]));
  observe('scope disposal',()=>{const a=new ALS({defaultValue:'d'});const scope=a.withScope('scope');const inside=a.getStore();const methods=[scope.constructor.name,scope.dispose===scope[Symbol.dispose]];scope[Symbol.dispose]();a.enterWith('later');scope.dispose();return[inside,a.getStore(),methods];});
  observe('scope symbol delegates',()=>{const scope=a.withScope('x');let called=false;scope.dispose=()=>{called=true;return 3;};return[scope[Symbol.dispose](),called];});
  a.disable();
  observe('disable',()=>{const a=new ALS({defaultValue:'d'});a.enterWith('x');return[a.disable(),a.getStore()];});
  observe('bound storage',()=>{const fn=a.run('bound',()=>ALS.bind(function(x){return[this.value,x,a.getStore()];}));return[fn.length,fn.call({value:2},8),a.getStore()];});
  observe('snapshot',()=>{const fn=a.run('snapshot',()=>ALS.snapshot());return[fn.length,fn((x)=>[a.getStore(),x],5),a.getStore()];});
  observe('resource snapshot',()=>{const a=new ALS({defaultValue:'d'});let resource;a.run('captured',()=>{resource=new AsyncResource('Fixture');});a.enterWith('later');const values=resource.runInAsyncScope(function(x){return[this===null,x,a.getStore(),executionAsyncId()===resource.asyncId()];},null,3);resource.emitDestroy();return[values,a.getStore()];});
  observe('resource bind receivers',()=>{const resource=new AsyncResource('Fixture');function f(x){return[this.value,x,executionAsyncId()===resource.asyncId()];}const dynamic=resource.bind(f),fixed=resource.bind(f,{value:7});return[dynamic.length,dynamic.call({value:2},3),fixed.call({value:4},5)];});
  observe('hook resource order',()=>{const events=[];let id;const hook=createHook({init(i,type,t,resource){if(type==='Fixture'){id=i;events.push(['init',t===executionAsyncId(),resource.asyncId()===i]);}},before(i){if(i===id)events.push('before');},after(i){if(i===id)events.push('after');}}).enable();const resource=new AsyncResource('Fixture');resource.runInAsyncScope(()=>events.push('body'));hook.disable();resource.emitDestroy();return events;});
  observe('event resource integration',()=>{const {EventEmitterAsyncResource}=require('node:events');const a=new ALS({defaultValue:'default'});const emitter=a.run('captured',()=>new EventEmitterAsyncResource({name:'Fixture'}));let seen;emitter.on('message',function(x){seen=[this===emitter,x,a.getStore(),executionAsyncId()===emitter.asyncId,emitter.asyncResource.eventEmitter===emitter];});const emitted=a.run('outer',()=>[emitter.emit('message',9),a.getStore()]);emitter.emitDestroy();return[seen,emitted,a.getStore()];});
  const pairs=await Promise.all(['first','second'].map(value=>a.run(value,async()=>{await Promise.resolve();return[a.getStore(),executionAsyncId(),triggerAsyncId()];})));
  out.push(['await stores and unobserved ids',pairs,a.getStore()]);
  const pending=a.run('pending',()=>Promise.resolve().then(()=>a.getStore()));a.disable();out.push(['disable preserves captured frame',await pending,a.getStore()]);
  const before=a.run('then',()=>Promise.resolve().then(()=>a.getStore()));a.enterWith('changed');out.push(['registration captures before enterWith',await before,a.getStore()]);a.disable();
  console.log(out.map(v=>JSON.stringify(v)).join('\n'));
}
main().catch(e=>{console.error(e);process.exitCode=1;});
