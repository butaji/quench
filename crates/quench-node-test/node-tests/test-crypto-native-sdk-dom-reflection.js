const ib=require('internal/test/binding').internalBinding;
const messaging=ib('messaging');const privateSymbol=ib('util').privateSymbols.transfer_mode_private_symbol;
const records=[];
function shape(error){return {name:error.name,message:error.message,code:error.code,tag:Object.prototype.toString.call(error),keys:Reflect.ownKeys(error).map(k=>typeof k==='symbol'?'symbol:'+String(k):k),symbols:Object.getOwnPropertySymbols(error).map(String),error:error instanceof Error,dom:error instanceof DOMException,quota:error instanceof QuotaExceededError,quotaValue:error.quota,requested:error.requested,prototype:error instanceof QuotaExceededError?Object.getPrototypeOf(error)===QuotaExceededError.prototype:Object.getPrototypeOf(error)===DOMException.prototype};}
records.push({name:'constructors',value:{dom:messaging.DOMException===DOMException,quota:messaging.QuotaExceededError===QuotaExceededError,subclass:Object.getPrototypeOf(QuotaExceededError.prototype)===DOMException.prototype}});
records.push({name:'dom',value:shape(new DOMException('material rejected','DataError'))});
const cause=new Error('actual cause');const withCause=new DOMException('failed',{name:'OperationError',cause});records.push({name:'cause',value:{...shape(withCause),causeIdentity:withCause.cause===cause,causeDescriptor:{enumerable:Object.getOwnPropertyDescriptor(withCause,'cause').enumerable,writable:Object.getOwnPropertyDescriptor(withCause,'cause').writable,configurable:Object.getOwnPropertyDescriptor(withCause,'cause').configurable}}});
records.push({name:'quota',value:shape(new QuotaExceededError('capacity',{quota:10,requested:20}))});
try{globalThis.crypto.getRandomValues(new Uint8Array(65537))}catch(e){records.push({name:'crypto-quota',value:shape(e)})}
const value={};value[privateSymbol]=2;records.push({name:'private-symbol',value:{type:typeof privateSymbol,read:value[privateSymbol],keys:Reflect.ownKeys(value).map(String),symbols:Object.getOwnPropertySymbols(value).map(String),enumerable:Object.prototype.propertyIsEnumerable.call(value,privateSymbol),descriptor:Object.getOwnPropertyDescriptor(value,privateSymbol)!==undefined}});
console.log(JSON.stringify(records,null,2));
