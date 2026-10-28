const get=require('internal/test/binding').internalBinding('util').getConstructorName;
const values=[function(){},()=>{},async function(){},function*(){},async function*(){}],out=[];
for(const value of values){const C=Object.getPrototypeOf(value).constructor;const name=Object.getOwnPropertyDescriptor(C,'name');Object.setPrototypeOf(value,null);Object.defineProperty(C,'name',{value:'Changed'+name.value});try{out.push(get(value))}finally{Object.defineProperty(C,'name',name)}}
console.log(JSON.stringify(out));
