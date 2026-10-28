const path=require('path');
class Example {
 #value=7;
 read(key){return this[key];}
 write(key,value){this[key]=value;}
 privateValue(){return this.#value;}
}
const value=new Example(),key='\0quench:private:'+__filename+':0:value',out=[];
try{out.push(['before',value.read(key)??null]);}catch(e){out.push(['before-error',e.name,e.message]);}
try{value.write(key,9);out.push(['after',value.privateValue(),value.read(key),Object.getOwnPropertyNames(value)]);}catch(e){out.push(['after-error',e.name,e.message]);}
console.log(JSON.stringify({complete:true,out}));
