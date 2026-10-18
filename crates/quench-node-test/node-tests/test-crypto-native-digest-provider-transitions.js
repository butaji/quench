const b=typeof setup==='function'?setup():require('internal/test/binding').internalBinding('crypto');
const set=typeof setNativeFips==='function'?setNativeFips:b.setFipsCrypto;const get=typeof getNativeFips==='function'?getNativeFips:b.getFipsCrypto;
const records=[];
function check(label,fn){try{const value=fn();records.push([label,'ok',value])}catch(error){records.push([label,error.name,error.code,error.message,error.opensslErrorStack??null])}}
for(const family of ['HashJob','HKDFJob'])for(const name of ['sha256','SHA256','dss1','md4','sha256-192'])for(const [before,after]of [[0,1],[1,0]]){
 check([family,name,before,after],()=>{set(before);const input=new Uint8Array([1]);const job=family==='HashJob'?new b.HashJob(1,name,input):new b.HKDFJob(1,name,input,new Uint8Array(),new Uint8Array(),32);set(after);const[error,value]=job.run();if(error)throw error;return[get(),Array.from(new Uint8Array(value))]});set(0);
}
if(typeof record==='function')record(JSON.stringify(records));else console.log(JSON.stringify(records));
