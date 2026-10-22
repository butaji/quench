'use strict';
const c=require('crypto');const records=[];const warnings=[];
process.on('warning',warning=>warnings.push([warning.name,warning.code,warning.message]));
function check(label,fn){try{records.push([label,fn()])}catch(e){records.push([label,e.name,e.code,e.message,e.reason,e.library])}}
check('public-inventory',()=>c.getHashes());
for(const name of require('internal/crypto/util').getHashes())check('incremental-'+name,()=>{
 const h=c.createHash(name).update('a');const copy=h.copy();
 const source=new Uint8Array([98,99]);h.update(source);source.fill(0);copy.update('bc');
 const result=h.digest('hex');return [result,copy.digest('hex')];
});
for(const name of ['sha1','sha256','sha3-256','md5','dss1','DSS1','sha256\0x'])for(const key of ['',Buffer.from('key'),c.createSecretKey(Buffer.from('key'))])check('hmac-'+name+'-'+(typeof key==='string'?'empty':Buffer.isBuffer(key)?'buffer':'key'),()=>{const h=c.createHmac(name,key).update('a').update('bc');return[h.digest('hex'),h.digest('hex')];});
for(const name of ['sha256','SHA256','RSA-SHA256','shake128','shake256','sha256\0x'])for(const encoding of ['hex','base64','buffer'])check('once-'+name+'-'+encoding,()=>{const out=c.hash(name,'abc',encoding);return Buffer.isBuffer(out)?{buffer:true,hex:out.toString('hex')}:out});
for(const [name,options]of [['sha256',{outputLength:1}],['unknown',undefined],['shake128',{outputLength:0}],['shake128',{outputLength:5}],['sha256',{functionName:Buffer.alloc(0)}]])check('options-'+name+'-'+JSON.stringify(options),()=>c.createHash(name,options).digest('hex'));
check('copy-default',()=>c.createHash('shake256',{outputLength:0}).copy().digest('hex'));
check('copy-change',()=>c.createHash('shake128',{outputLength:0}).copy({outputLength:5}).digest('hex'));
check('streams',()=>{const h=c.createHash('sha256');h.write('a');h.end('bc');const first=h.read();return[first.toString('hex'),h.digest('hex')]});
check('hmac-stream',()=>{const h=c.createHmac('sha256','key');h.write('a');h.end('bc');return[h.read().toString('hex'),h.digest('hex')]});
for(const method of ['update','digest','copy'])check('finalized-'+method,()=>{const h=c.createHash('sha256');h.digest();return h[method]('abc')});
check('unknown-hmac',()=>c.createHmac('unknown','key'));
check('unknown-once',()=>c.hash('unknown','abc'));
check('badlen-once',()=>c.hash('sha256','abc',{outputLength:1}));
check('source-class',()=>{const h=c.Hash('sha256');const m=c.Hmac('sha256','key');return[h instanceof c.Hash,m instanceof c.Hmac,c.createHash.length,c.createHmac.length,c.createHash.toString(),c.createHmac.toString()]});
process.nextTick(()=>console.log(JSON.stringify({records,warnings})));
