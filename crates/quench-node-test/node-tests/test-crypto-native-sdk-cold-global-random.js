const crypto=globalThis.crypto;
const records=[];
function check(name,fn){try{records.push({name,value:fn()})}catch(e){records.push({name,error:e.name,message:e.message})}}
check('cold',()=>({name:crypto.constructor.name,source:crypto===require('internal/crypto/webcrypto').crypto,module:crypto===require('node:crypto').webcrypto,subtle:crypto.subtle===require('node:crypto').subtle,random:crypto.getRandomValues(new Uint8Array(4)).length}));
check('uuid',()=>{const a=crypto.randomUUID();const b=crypto.randomUUID();return{format:/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(a),distinct:a!==b}});
check('float',()=>crypto.getRandomValues(new Float32Array(4)));
check('quota',()=>crypto.getRandomValues(new Uint8Array(65537)));
console.log(JSON.stringify(records,null,2));
