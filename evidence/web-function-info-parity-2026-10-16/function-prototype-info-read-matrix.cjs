const {internalBinding}=require('internal/test/binding');
const name=internalBinding('util').getConstructorName;
const reads={name:f=>f.name,length:f=>f.length,missing:f=>f.missing,own:f=>f.x,reflect:f=>Reflect.get(f,'name'),computed:(f,k)=>f[k],descriptor:f=>Object.getOwnPropertyDescriptor(f,'name'),inherited:(f,k,p)=>p.name};
const mutations={none(){},nameData(f){Object.defineProperty(f,'name',{value:'anonymous'})},lengthData(f){Object.defineProperty(f,'length',{value:0})},lengthDescriptor(f){Object.defineProperty(f,'length',{configurable:false})},lengthGetter(f){Object.defineProperty(f,'length',{get(){return 0}})},deleteLength(f){delete f.length}};
const out=[];
for(const [mutation,mutate]of Object.entries(mutations))for(const [read,observe]of Object.entries(reads)){
 const F=Function('this.x=1');F.x=1;mutate(F);const p=Object.create(F),a=new F(),b=new F();Object.setPrototypeOf(b,null);
 const before=[name(a),name(b)];observe(F,'name',p);out.push([mutation,read,before,[name(a),name(b)]]);
}
console.log(JSON.stringify(out));
