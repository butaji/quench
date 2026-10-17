class Base {constructor(object){return object}}
function make(){return class extends Base {#value;constructor(object,value){super(object);this.#value=value}get(){return this.#value}static has(object){return #value in object}}}
const A=make(),B=make(),object={};new A(object,1);new B(object,2);
console.log(JSON.stringify([A.prototype.get.call(object),B.prototype.get.call(object),A.has(object),B.has(object),Reflect.ownKeys(object)]));
