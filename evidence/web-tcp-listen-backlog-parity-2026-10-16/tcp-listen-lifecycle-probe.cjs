const net=require('net'),out=[];
function snapshot(server){let address;try{address=server.address();address=address===null?null:typeof address==='object'?'bound':'path'}catch(error){address=['throw',error.name,error.message]}return [server.listening,address,server._listeningId]}
async function valid(name,host){await new Promise(resolve=>{const server=net.createServer(),options={port:0};if(host!==undefined)options.host=host;out.push([name,'new',snapshot(server)]);server.listen(options,()=>{out.push([name,'listening',snapshot(server)]);server.close(error=>{out.push([name,'closed',error&&error.code,snapshot(server)]);resolve()})});out.push([name,'returned',snapshot(server)])})}
(async()=>{await valid('wildcard',undefined);await valid('host','127.0.0.1');console.log(JSON.stringify(out))})();
