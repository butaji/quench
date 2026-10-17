const {internalBinding}=require('internal/test/binding'),util=require('util');
const nativeName=internalBinding('util').getConstructorName;
Error.prepareStackTrace=(_,frames)=>frames;
const Kinds=[Function,(async function(){}).constructor,(function*(){}).constructor,(async function*(){}).constructor];
const mutations={
 default(){},read(fn){void fn.name;Object.getOwnPropertyDescriptor(fn,'name')},
 readonly(fn){fn.name='ignored'},changed(fn){Object.defineProperty(fn,'name',{value:'changed'})},
 empty(fn){Object.defineProperty(fn,'name',{value:''})},nonstring(fn){Object.defineProperty(fn,'name',{value:7})},
 getter(fn){Object.defineProperty(fn,'name',{get(){hooks++;throw new Error('name hook')}})},
 deleted(fn){delete fn.name},frozen(fn){Object.freeze(fn)},sealed(fn){Object.seal(fn)},
 prototype(fn){Object.setPrototypeOf({},fn)}
};
let hooks=0;const out=[],keepAlive=[];
function inspect(value){try{return ['value',util.inspect(value)]}catch(error){return ['throw',error.name,error.message]}}
async function invoke(fn){const value=fn();return typeof value?.next==='function'?(await value.next()).value:await value}
(async()=>{
 for(const Kind of Kinds)for(const [mode,mutate]of Object.entries(mutations)){
  const fn=Kind('return [new Error().stack[0],require("util").getCallSites(1)[0]]');
  mutate(fn);const [frame,site]=await invoke(fn);
  out.push([Kind.name,mode,frame.getFunctionName(),site.functionName,hooks]);
 }
 for(const [mode,mutate]of Object.entries(mutations)){
  const Maker=Function('this.x=1');keepAlive.push(Maker);mutate(Maker);const a=new Maker(),b=new Maker();Object.setPrototypeOf(b,null);
  out.push([mode,nativeName(a),nativeName(b),inspect(a),inspect(b),hooks]);
 }
 const fn=Function('return new Error().stack[0]'),frame=fn();
 Object.defineProperty(fn,'name',{value:'after-capture'});out.push(['retained',frame.getFunctionName()]);
 console.log(JSON.stringify([out,keepAlive.length]));
})();
