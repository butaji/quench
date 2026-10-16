const net=require('net'),dns=require('dns'),out=[],original=dns.lookup;
async function check(name,kind){await new Promise(resolve=>{
 const events=[],server=net.createServer();
 dns.lookup=(host,options,callback)=>process.nextTick(()=>callback(null,{[Symbol.iterator](){return {next(){events.push('next');return {done:false,value:{get address(){events.push('address');if(kind==='getter'){const error=new RangeError('address getter');error.token=Symbol('getter-root');throw error}return '127.0.0.1'}}}},return(){events.push('return');if(typeof gc==='function')gc();throw new SyntaxError('iterator return')}}}}));
 process.setUncaughtExceptionCaptureCallback(error=>{process.setUncaughtExceptionCaptureCallback(null);out.push([name,error.name,error.message,error.token&&error.token.description,events,server.listening]);server.close(closeError=>{out.push([name,'close',closeError&&closeError.code]);resolve()})});
 server.listen({port:0,host:'test.invalid'});events.push('returned');
})}
(async()=>{try{await check('getter','getter');await check('return','return');console.log(JSON.stringify(out))}finally{dns.lookup=original;process.setUncaughtExceptionCaptureCallback(null)}})();
