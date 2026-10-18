'use strict';const net=require('net');const assert=require('assert');let read=0,ended=0;
const server=net.createServer(socket=>{
 Object.defineProperty(socket._handle,'onread',{value(){throw new Error('raw callback wrongly selected after public Socket admission')},configurable:true,writable:true});
 socket.on('data',data=>{read++;assert.equal(data.toString(),'owned');socket.write(data)});
 socket.on('end',()=>{ended++;socket.end()});
}).listen(0,'127.0.0.1',()=>{const client=net.connect(server.address().port,'127.0.0.1',()=>client.end('owned'));client.on('data',data=>assert.equal(data.toString(),'owned'));client.on('close',()=>server.close(()=>{assert.equal(read,1);assert.equal(ended,1);console.log('public Socket owns accepted data and EOF delivery')}));});
