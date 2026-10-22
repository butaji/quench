
Error.prepareStackTrace=(_,frames)=>frames;
const changes={name:f=>f.name,length:f=>f.length,reflect:f=>Reflect.get(f,'name'),computed:(f,k)=>f[k]};
for(const [kind,read] of Object.entries(changes)) {
 const fn=Function('return new Error().stack[0]');Object.setPrototypeOf({},fn);
 if(fn().getFunctionName()!=='anonymous')throw Error('setup '+kind);
 read(fn,'name');const expected=kind==='name'||kind==='length'?'eval':'anonymous';
 if(fn().getFunctionName()!==expected)throw Error('lookup '+kind);
}
const fn=Function('return new Error().stack[0]');delete fn.length;Object.setPrototypeOf({},fn);void fn.name;
if(fn().getFunctionName()!=='anonymous')throw Error('deleted length layout');
