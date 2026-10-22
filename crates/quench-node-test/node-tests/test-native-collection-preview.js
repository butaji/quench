'use strict';
const {previewEntries}=require('internal/test/binding').internalBinding('util');
const key={k:1},value={v:2};const map=new Map([[key,value],[3,4]]),set=new Set([key,3]);
for(const [name,item]of[['map',map],['set',set],['weakmap',new WeakMap([[key,value]])],['weakset',new WeakSet([key])],['mapkeys',map.keys()],['mapvalues',map.values()],['mapentries',map.entries()],['setvalues',set.values()],['setentries',set.entries()],['array',[1,2][Symbol.iterator]()],['string','ab'[Symbol.iterator]()],['ordinary',{}]]){
 console.log(name,JSON.stringify(previewEntries(item)),JSON.stringify(previewEntries(item,true)));
 if(item.next){console.log('next',JSON.stringify(item.next()));console.log('remaining',JSON.stringify(previewEntries(item,true)));}
}

const changing=new Map([[1,'a'],[2,'b'],[3,'c']]);const iterator=changing.entries();iterator.next();changing.delete(2);changing.set(4,'d');
console.log('deleted slots',JSON.stringify(previewEntries(iterator,false)),JSON.stringify(iterator.next()));
changing.clear();changing.set(5,'e');console.log('clear',JSON.stringify(previewEntries(iterator,true)),JSON.stringify(iterator.next()));
iterator.next();changing.set(6,'f');console.log('exhausted',JSON.stringify(previewEntries(iterator,true)),JSON.stringify(iterator.next()));
let traps=0;const proxy=new Proxy(map,{get(){traps++;throw Error('trap')}});console.log('proxy',previewEntries(proxy)===undefined,traps);

console.log("metadata",previewEntries.name,previewEntries.length,Function.prototype.toString.call(previewEntries),JSON.stringify(Object.getOwnPropertyDescriptor(previewEntries,"name")),Object.hasOwn(previewEntries,"prototype"));
