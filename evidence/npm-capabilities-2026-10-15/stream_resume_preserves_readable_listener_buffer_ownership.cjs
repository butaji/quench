
'use strict';
const {Readable}=require('node:stream');
const out=[];
const stream=new Readable({read(){}});
stream.on('readable',()=>{});
stream.on('data',chunk=>out.push(['data',chunk.toString()]));
stream.resume();
stream.push('retained');
process.nextTick(()=>{
 out.push(['state',stream.readableFlowing,stream.isPaused(),stream.readableLength]);
 out.push(['read',stream.read()?.toString()]);
 console.log(JSON.stringify(out));
});
