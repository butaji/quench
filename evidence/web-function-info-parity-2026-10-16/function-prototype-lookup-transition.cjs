const {internalBinding}=require('internal/test/binding'),name=internalBinding('util').getConstructorName;
for(const op of ['name','length','missing','dynamic','has','reflect','inherited']){
 const F=Function(''),p=Object.setPrototypeOf({},F),o=new F();Object.setPrototypeOf(o,null);const before=name(o);
 if(op==='name')void F.name;if(op==='length')void F.length;if(op==='missing')void F.zzz;
 if(op==='dynamic'){const k=process.argv[2]||'name';void F[k]}
 if(op==='has')void('name' in F);if(op==='reflect')Reflect.get(F,'length');if(op==='inherited')void p.zzz;
 console.log(JSON.stringify([op,before,name(o)]));
}
