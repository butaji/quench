const rows=[];
function capture(name,fn){try{rows.push({name,result:fn()});}catch(e){rows.push({name,error:{name:e.name,message:e.message}});}}
class Named{
 #field=1; #method(){return 2}; get #accessor(){return this.#field};set #accessor(x){this.#field=x};get #readonly(){return this.#field};set #writeonly(x){this.#field=x};
 read(){return this.#field};write(x){this.#field=x;return this.#field};method(){return this.#method()};getAccessor(){return this.#accessor};setAccessor(x){this.#accessor=x;return this.#field};setMethod(){this.#method=3};setReadonly(){this.#readonly=3};getWriteonly(){return this.#writeonly};
 static #field=1;static #method(){return 2};static get #accessor(){return this.#field};static set #accessor(x){this.#field=x};
 static read(){return this.#field};static write(x){this.#field=x;return this.#field};static method(){return this.#method()};static getAccessor(){return this.#accessor};static setAccessor(x){this.#accessor=x;return this.#field};static setMethod(){this.#method=3};
}
for(const key of ['read','write','method','getAccessor','setAccessor','setMethod','setReadonly','getWriteonly'])for(const [name,value]of[['instance',new Named()],['object',{}],['null',null],['undefined',undefined],['number',1],['proxy',new Proxy(new Named(),{})]])capture('instance-'+key+'-'+name,()=>Named.prototype[key].call(value,7));
for(const key of ['read','write','method','getAccessor','setAccessor','setMethod'])for(const[name,value]of[['class',Named],['object',{}],['null',null],['undefined',undefined],['number',1],['proxy',new Proxy(Named,{})]])capture('static-'+key+'-'+name,()=>Named[key].call(value,9));
console.log(JSON.stringify({complete:true,cases:rows.length,rows}));
