Error.prepareStackTrace=(_,frames)=>frames[0];
function Capture(){return new Error().stack}
function StrictCapture(){'use strict';return new Error().stack}
function Other(){}
const Unnamed=(0,function(){return new Error().stack});
function Receiver(){}
let calls=0;const proxy=new Proxy({},{get(){calls++;throw Error('proxy get')},getPrototypeOf(){calls++;throw Error('proxy prototype')}});
const revoked=Proxy.revocable(()=>{},{});revoked.revoke();
const tagged=Object.create(null);tagged[Symbol.toStringTag]='Tagged';
const values=[undefined,null,{},[],1,'x',true,Symbol('s'),1n,Receiver,Array,Receiver.bind(null),(async()=>{}).bind(null),proxy,revoked.proxy,tagged];
const out=[];
function row(site){const type=site.getTypeName();return[site.getFunctionName(),type,type===null,site.isConstructor(),site.getFunction()===Capture,site.getFunction()===undefined,site.getThis()===undefined,site.isConstructor()&&site.toString().startsWith('new ')]}
for(const fn of [Capture,StrictCapture])for(const value of values)out.push(row(fn.call(value)));
for(const C of [Capture,StrictCapture,Unnamed])for(let i=0;i<4;i++){out.push(row(new C),row(Reflect.construct(C,[],Other)))}
Object.defineProperty(Receiver,'name',{get(){calls++;throw Error('receiver name')}});out.push(row(Capture.call(Receiver)),calls);
delete Receiver.name;Object.setPrototypeOf(Receiver,{name:'InheritedReceiver'});out.push(row(Capture.call(Receiver)),calls);
const target=new Proxy(Other,{get(object,key,receiver){if(key==='prototype')out.push(row(new Error().stack));return Reflect.get(object,key,receiver)}});
out.push(row(Reflect.construct(Capture,[],target)));
console.log(JSON.stringify(out));
