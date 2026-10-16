'use strict';
const errors=require('internal/errors'),util=require('util'),dgram=require('dgram');
const Ctor=errors.codes.ERR_SOCKET_BUFFER_SIZE;
console.log('default hook',typeof Error.prepareStackTrace,Object.hasOwn(Error,'prepareStackTrace'));
const socket=dgram.createSocket('udp4');
let error;try{socket.getSendBufferSize()}catch(e){error=e}
console.log('identity',error instanceof Ctor,error instanceof Error,error[errors.kIsNodeError]);
console.log('header',error.stack.split('\n')[0],error.toString());
for(const name of ['name','message','info','errno','syscall']){const d=Object.getOwnPropertyDescriptor(error,name);console.log('descriptor',name,d.enumerable,d.configurable,d.writable,typeof d.get,typeof d.set)}
error.errno=42;error.syscall='changed';console.log('context',error.info.errno,error.info.syscall);
error.stack=error.stack.split('\n')[0];console.log('inspect',util.inspect(error));
const originalBrand=errors.kIsNodeError;errors.kIsNodeError=Symbol('replacement');
for(const operation of [()=>require('fs').readFileSync(null)]){try{operation()}catch(e){console.log('native coded',e.name,e.code,e[originalBrand],e[errors.kIsNodeError],e.stack.split('\n')[0])}}
errors.kIsNodeError=originalBrand;
const overridden=new Error('override');errors.overrideStackTrace.set(overridden,(e,sites)=>'override:'+e.message+':'+Array.isArray(sites));console.log('internal override',overridden.stack,overridden.stack);
const reason={};let calls=0;
Error.prepareStackTrace=(e,sites)=>{calls++;if(e.message==='throw')throw reason;return {error:e,array:Array.isArray(sites)}};
const custom=new Error('custom'),value=custom.stack;console.log('user hook',value.error===custom,value.array,custom.stack===value,calls);
const throwing=new Error('throw');try{throwing.stack}catch(e){console.log('throw identity',e===reason)}
Error.prepareStackTrace=undefined;
console.log('retry',throwing.stack.split('\n')[0],calls);
errors.setInternalPrepareStackTrace((e,sites)=>'policy:'+e.message+':'+Array.isArray(sites));console.log('internal policy',new Error('policy').stack);
errors.setInternalPrepareStackTrace(errors.defaultPrepareStackTrace);
