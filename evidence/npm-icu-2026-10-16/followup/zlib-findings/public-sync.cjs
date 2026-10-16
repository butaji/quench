const z=require('node:zlib');let seed=2612;const random=Array.from({length:2000},()=>{seed=(Math.imul(seed,1664525)+1013904223)>>>0;return seed>>>24;});
const data=[[],[65],Array.from({length:256},(_,i)=>i),Array.from(Buffer.from('hello world'.repeat(100))),random],rows=[];
for(const mode of ['gzip','deflate','deflateRaw'])for(let level=-1;level<=9;level++)for(let index=0;index<data.length;index++){
 const bytes=z[mode+'Sync'](Buffer.from(data[index]),{level});rows.push([mode,level,index,Array.from(bytes)]);
}
console.log(JSON.stringify({complete:true,cases:rows.length,rows}));
