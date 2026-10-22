const {internalBinding}=require('internal/test/binding'),name=internalBinding('util').getConstructorName;
for(const order of ['read-before','read-after','descriptor-after','read-after-twice','read-inherited','reflect-after','ownkeys-after']){
 const F=Function(''); if(order==='read-before')void F.name;
 const p=Object.setPrototypeOf({},F),o=new F();Object.setPrototypeOf(o,null);
 const before=name(o);
 if(order==='read-after'||order==='read-after-twice')void F.name;
 if(order==='read-after-twice')void F.name;
 if(order==='descriptor-after')Object.getOwnPropertyDescriptor(F,'name');
 if(order==='read-inherited')void p.name;
 if(order==='reflect-after')Reflect.get(F,'name');
 if(order==='ownkeys-after')Reflect.ownKeys(F);
 console.log(JSON.stringify([order,before,name(o)]));
}
