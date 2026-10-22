class ReturnValue {constructor(object){return object}}
function make(){return class extends ReturnValue {
 #value;
 constructor(object,value){super(object);this.#value=value}
 read(){return this.#value}
 write(value){this.#value=value}
 evaluate(){return eval('this.#value')}
 static has(object){return #value in object}
}}
const out=[];
function outcome(fn){try{return ['value',fn()]}catch(error){return ['throw',error.name,error.message]}}
function check(name,object){
 const A=make(),B=make();
 try{
  new A(object,1);const initial=[A.has(object),B.has(object)];new B(object,2);
  const values=[A.prototype.read.call(object),B.prototype.read.call(object)];A.prototype.write.call(object,3);
  let duplicate;try{new A(object,4)}catch(error){duplicate=[error.name,error.message]}
  out.push([name,initial,values,[A.prototype.read.call(object),B.prototype.read.call(object)],duplicate,[outcome(()=>A.prototype.evaluate.call(object)),outcome(()=>B.prototype.evaluate.call(object))],A.has(object),B.has(object)]);
 }catch(error){out.push([name,'throw',error.name,error.message])}
}
check('plain',{});check('array',[]);check('function',function target(){});check('null-prototype',Object.create(null));
check('frozen',Object.freeze({}));check('sealed',Object.seal({}));check('nonextensible',Object.preventExtensions({}));
const handler={};for(const method of ['get','set','defineProperty','getOwnPropertyDescriptor','getPrototypeOf','isExtensible'])handler[method]=()=>{throw Error('unexpected '+method)};
check('proxy',new Proxy({},handler));const revoked=Proxy.revocable({},{});revoked.revoke();check('revoked-proxy',revoked.proxy);
class Method extends ReturnValue {#method(){return 7}read(){return this.#method()}}
class Accessor extends ReturnValue {get #value(){return 8}read(){return this.#value}}
for(const [name,Type]of [['method',Method],['accessor',Accessor]]){
 const object=Object.freeze({});try{new Type(object);out.push([name,Type.prototype.read.call(object)])}catch(error){out.push([name,'throw',error.name,error.message])}
}
function staticClass(value){return class {static #value=value;static read(){return this.#value}static write(value){this.#value=value}}}
const A=staticClass(1),B=staticClass(2);A.write(3);out.push(['static',A.read(),B.read()]);
const C=make(),object={};new C(object,17);const clone=structuredClone(object);out.push(['clone',C.has(clone),Reflect.ownKeys(object),Reflect.ownKeys(clone)]);
console.log(JSON.stringify(out));
