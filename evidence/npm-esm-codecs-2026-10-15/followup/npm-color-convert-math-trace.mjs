import api from '/workspace/quench/tests/npm-top300/node_modules/color-convert/index.js';
const samples={rgb:[[0,0,0],[255,255,255],[255,0,0],[12.5,44,200]],hsl:[[0,100,50],[120,100,50],[240,30,80]],hsv:[[0,100,100],[180,50,75]],hwb:[[60,10,20],[0,100,0]],cmyk:[[0,100,100,0],[25,50,75,10]],xyz:[[41.24,21.26,1.93],[95.047,100,108.883]],lab:[[50,0,0],[50,25,-35]],oklab:[[50,0,0],[70,10,-20]],oklch:[[50,20,60]],lch:[[50,30,60]],hex:['FF0000','12abef'],keyword:['red','rebeccapurple','blue'],ansi16:[31,90,97],ansi256:[0,16,196,255],hcg:[[0,100,0],[180,50,25]],apple:[[65535,0,0],[0,65535,0]],gray:[[0],[50],[100]]};
const traces=[];let currentId;
for(const name of ['atan2','cbrt','pow','sin','cos','sqrt','atan','acos','exp','log']) {
 const original=Math[name];Math[name]=(...args)=>{const result=original(...args);traces.push({id:currentId,name,args,result});return result;};
}
const rows=[];
for(const from of Object.keys(api))for(const to of Object.keys(api[from])){
 const fn=api[from][to];if(typeof fn!=='function')continue;
 if(!(from in samples))throw Error(`missing model ${from}`);
 for(const raw of [false,true])for(const input of samples[from]){
   const convert=raw?fn.raw:fn;currentId=`${from}.${to}${raw?".raw":""}`;const result=convert(input);rows.push({id:`${from}.${to}${raw?'.raw':''}`,input,result});
 }
}
console.log(JSON.stringify({complete:true,functions:new Set(rows.map(r=>r.id)).size,cases:rows.length,rows,traces}));
