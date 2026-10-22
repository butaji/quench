const path=require('path');
class Example {
 #value=7;
 read(key){return this[key];}
 write(key,value){this[key]=value;}
 privateValue(){return this.#value;}
}
const value=new Example(),keys=[
 '\0quench:private:'+__filename+':0:value',
 '\0quench:private:'+__filename+':0:field::value',
 '\0plain-public-key',
],out=[];
for(const key of keys){
 try{out.push(['before',key,value.read(key)??null]);}catch(e){out.push(['before-error',key,e.name,e.message]);}
 try{value.write(key,9);out.push(['after',key,value.privateValue(),value.read(key),Object.getOwnPropertyNames(value)]);}catch(e){out.push(['after-error',key,e.name,e.message]);}
}
console.log(JSON.stringify({complete:true,out}));
