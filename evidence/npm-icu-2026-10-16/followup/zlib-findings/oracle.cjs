const fs=require('fs'),zlib=require('zlib');
const rows=fs.readFileSync(0,'utf8').trim().split('\n').map(JSON.parse);
for(const row of rows){
 const engine=new zlib[row.mode]({level:row.level,chunkSize:64}),native=engine._handle,close=native.close;
 native.close=()=>{};const out=[];
 try{for(const part of row.chunks){const result=engine._processChunk(Buffer.from(part.bytes),part.flush);engine._handle=native;out.push({bytes:[...result],consumed:engine.bytesWritten});}}
 finally{native.close=close;engine._handle=native;engine.close();}
 console.log(JSON.stringify(out));
}
