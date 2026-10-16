'use strict';const assert=require('assert');const net=require('net');const {internalBinding}=require('internal/test/binding');
const {TCP}=internalBinding('tcp_wrap');const b=internalBinding('stream_wrap');
for(const [name,index]of [['kReadBytesOrError',0],['kArrayBufferOffset',1],['kBytesWritten',2],['kLastWriteWasAsync',3],['kLastWriteErr',4]])assert.equal(b[name],index);
assert(b.streamBaseState instanceof Int32Array);assert.equal(b.streamBaseState.length,5);
const listener=new TCP(1);assert.equal(listener.bind('127.0.0.1',0),0);const out={};assert.equal(listener.getsockname(out),0);
let read=0,eof=0,closed=0;listener.onconnection=function(status,handle){assert.equal(status,0);assert.strictEqual(this,listener);
 handle.onread=function(arrayBuffer){assert.strictEqual(this,handle);const nread=b.streamBaseState[b.kReadBytesOrError];const offset=b.streamBaseState[b.kArrayBufferOffset];
  if(nread>0){read++;assert(arrayBuffer instanceof ArrayBuffer);const bytes=Buffer.from(arrayBuffer,offset,nread);assert.equal(bytes.toString(),'hello world');
   const req=new b.WriteWrap();req.async=false;req.oncomplete=()=>assert.fail('synchronous write must not invoke oncomplete');
   assert.equal(handle.writeBuffer(req,bytes),0);assert.equal(handle.writeQueueSize,0);assert.equal(b.streamBaseState[b.kBytesWritten],11);assert.equal(b.streamBaseState[b.kLastWriteWasAsync],0);assert.equal(b.streamBaseState[b.kLastWriteErr],0);assert.equal(req.async,false);console.log('intrinsic ArrayBuffer onread; real synchronous native write; state updated; no completion request');
  }else{eof++;assert.equal(arrayBuffer,undefined);assert.equal(nread,-4095);handle.close(()=>{closed++;console.log('EOF state=-4095; native close complete')});listener.close();}
 };assert.equal(handle.readStart(),0);
};assert.equal(listener.listen(128),0);const socket=net.connect(out.port,'127.0.0.1');socket.on('connect',()=>socket.end('hello world'));socket.on('data',bytes=>assert.equal(bytes.toString(),'hello world'));socket.on('error',err=>{throw err});process.on('exit',()=>{assert.equal(read,1);assert.equal(eof,1);assert.equal(closed,1)});
