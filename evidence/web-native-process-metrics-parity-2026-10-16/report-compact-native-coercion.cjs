const {internalBinding}=require('internal/test/binding');
const binding=internalBinding('report');
const original=binding.getCompact(),result=[];
for(const value of [undefined,null,false,true,0,-0,NaN,1,'','x',0n,1n,Symbol('x'),{},[],new Proxy({},{get(){throw new Error('unexpected coercion')}})]){
 try { const returned=binding.setCompact(value);result.push([typeof returned,binding.getCompact()]); }
 catch(error){result.push([error.name,error.message]);}
}
binding.setCompact(original);
console.log(JSON.stringify(result));
