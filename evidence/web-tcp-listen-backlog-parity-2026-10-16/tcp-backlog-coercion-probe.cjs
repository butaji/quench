const net=require('net'),out=[];
for(const [name,backlog]of [['symbol',Symbol('backlog')],['bigint',1n],['throwing',{valueOf(){throw new RangeError('backlog conversion')}}]]) {
 const server=net.createServer().unref();
 try{server.listen({port:0,host:'127.0.0.1',backlog});out.push([name,'accepted'])}catch(error){out.push([name,error.name,error.message])}
}
console.log(JSON.stringify(out));
