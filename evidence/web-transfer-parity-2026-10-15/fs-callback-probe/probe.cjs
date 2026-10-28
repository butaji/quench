const fs=require('node:fs');
let count=0;const marker={};
process.on('uncaughtException',e=>console.log('uncaught',e===marker,count));
for(const name of ['stat','lstat','statfs'])fs[name]('/',function(error,result){count++;console.log(name,count,error===null,!!result);throw marker});
