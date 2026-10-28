const net=require('net'),dns=require('dns'),out=[];
function snapshot(server){const address=server.address();return [server.listening,address===null?null:'bound',server._listeningId]}
async function listening(name,invoke){await new Promise(resolve=>{const server=net.createServer();out.push([name,'new',snapshot(server)]);const callback=()=>{out.push([name,'listening',snapshot(server)]);server.close(error=>{out.push([name,'closed',error&&error.code,snapshot(server)]);resolve()})};out.push([name,'same',invoke(server,callback)===server]);out.push([name,'returned',snapshot(server)])})}
async function closing(name,listen){await new Promise(resolve=>{const server=net.createServer();if(listen)server.listen({port:0,host:'127.0.0.1'},()=>out.push([name,'unexpected-listening']));server.close(error=>{out.push([name,error&&error.code,snapshot(server)]);resolve()})})}
(async()=>{
 await listening('wildcard',(server,callback)=>server.listen(0,callback));
 await listening('host',(server,callback)=>server.listen(0,'127.0.0.1',callback));
 await listening('positional',(server,callback)=>server.listen(0,'127.0.0.1',7,callback));
 await listening('port-backlog',(server,callback)=>server.listen(0,7,callback));
 await closing('new-close',false);await closing('cancel',true);
 await new Promise(resolve=>{const server=net.createServer();server.listen({port:0,host:'127.0.0.1'},()=>server.close(first=>{out.push(['twice','first',first&&first.code,snapshot(server)]);server.close(second=>{out.push(['twice','second',second&&second.code,snapshot(server)]);resolve()})}))});
 const observed=net.createServer();observed.once=function(event,callback){const descriptor=Object.getOwnPropertyDescriptor(callback,'prototype');out.push(['close-wrapper',event,callback.name,callback.length,Reflect.ownKeys(callback),descriptor.writable,descriptor.enumerable,descriptor.configurable]);out.push(['close-wrapper-result',callback()===undefined]);return this};observed.close(error=>{out.push(['close-wrapper-error',error.name,error.code,error.message,String(error),Object.getPrototypeOf(error)===Error.prototype]);return {ignored:true}});
 const original=dns.lookup;
 try {
  dns.lookup=function(host,options,callback){out.push(['lookup',host,options.all,callback.name,callback.length,Reflect.ownKeys(callback)]);process.nextTick(()=>{if(typeof gc==='function')gc();callback(null,{[Symbol.iterator](){let index=0;return {next(){return index++===0?{value:{address:'fe80::1',family:6},done:false}:{value:{address:'127.0.0.1',family:4},done:false}},return(){out.push(['iterator-close']);return {}}}}})})};
  await listening('custom-dns',(server,callback)=>server.listen({port:0,host:'test.invalid'},callback));
 } finally {dns.lookup=original}
 for(const name of ['listen','close','address']){const fn=net.Server.prototype[name];const descriptor=Object.getOwnPropertyDescriptor(fn,'prototype');out.push(['method',name,fn.name,fn.length,Reflect.ownKeys(fn),descriptor.writable,descriptor.enumerable,descriptor.configurable,fn.prototype.constructor===fn])}
 const descriptor=Object.getOwnPropertyDescriptor(net.Server.prototype,'listening');out.push(['getter',descriptor.get.name,descriptor.get.length,descriptor.enumerable,descriptor.configurable,Reflect.ownKeys(descriptor.get),new descriptor.get() instanceof descriptor.get]);
 console.log(JSON.stringify(out));
})();
