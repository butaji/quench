Error.prepareStackTrace=(_,frames)=>frames;
const C=Object.getPrototypeOf(new Error().stack[0]).constructor,calls=[],out=[];
const target=new Proxy(function(){},{get(object,key,receiver){calls.push(String(key));return Reflect.get(object,key,receiver)}});
for(const invoke of [()=>C(),()=>new C,()=>Reflect.construct(C,[],target)])try{invoke()}catch(error){out.push([error.name,error.message])}
console.log(JSON.stringify([out,calls]));
