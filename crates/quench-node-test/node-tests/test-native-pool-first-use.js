// Linux native resource observation; /proc reports actual OS workers.
const assert=require('assert');const fs=require('fs');const dns=require('dns');const crypto=require('crypto');
const count=()=>fs.readdirSync('/proc/self/task').length;
const initial=count();
process.env.UV_THREADPOOL_SIZE='1';
dns.lookup('localhost',()=>{});
const first=count();
process.env.UV_THREADPOOL_SIZE='3';
crypto.generateKey('hmac',{length:128},()=>{});
const second=count();
assert.strictEqual(first-initial,1);assert.strictEqual(second-first,0);
console.log('native-pool-lazy-first-use-ok');
