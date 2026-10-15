const fs=require('fs');
for(const args of [[-1,{},0,1,0],[-1,Buffer.alloc(1),0,0,0],[{},Buffer.alloc(1),0,0,0],[-1,Buffer.alloc(1),0,1,-1],[-1,Buffer.alloc(1),0,1,0n],[-1,Buffer.alloc(1),0,1,-1n],[-1,Buffer.alloc(1),0,1,'0'],[-1,Buffer.alloc(1),0,undefined,0],[-1,Buffer.alloc(1),0],[-1,Buffer.alloc(1),null]]){
try{console.log(JSON.stringify(['value',fs.readSync(...args)]))}catch(e){console.log(JSON.stringify(['error',e.name,e.code,e.message,Reflect.ownKeys(e).filter(x=>x!=='stack')]))}
}
