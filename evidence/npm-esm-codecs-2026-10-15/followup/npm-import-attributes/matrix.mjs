const cases = [undefined, {}, {type:'json'}, {type:'javascript'}, {type:'unsupported'}, {other:'x'}, {type:'json',other:'x'}, {other:'x',type:'unsupported'}, {type:undefined}, {type:null}, {type:1}];
const rows=[];
for(const specifier of ['./value.json','./value.mjs','./value.cjs','node:path']) for(let i=0;i<cases.length;i++) {
  try {const result = await import(specifier, cases[i]===undefined?undefined:{with:cases[i]}); rows.push({specifier,case:i,ok:true,keys:Object.keys(result),value:specifier==='node:path'?typeof result.default:result.default});}
  catch(e){ rows.push({specifier,case:i,name:e.name,code:e.code??null,message:e.message}); }
}
console.log(JSON.stringify({complete:true,rows}));
