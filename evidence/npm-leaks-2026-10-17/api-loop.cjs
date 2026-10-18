const r=()=>Math.round(process.memoryUsage.rss()/1048576);
const {EventEmitter}=require('events'); const crypto=require('crypto'); const zlib=require('zlib'); const {Readable}=require('stream');
const ee=new EventEmitter(); ee.on('x',()=>{});
const cases={
 emit:()=>ee.emit('x',1),
 hash:()=>crypto.createHash('sha256').update('abc').digest('hex'),
 randomBytes:()=>crypto.randomBytes(16),
 deflate:()=>zlib.deflateSync('hello hello hello'),
 bufConcat:()=>Buffer.concat([Buffer.from('a'),Buffer.from('b')]),
 bufToString:()=>Buffer.from('hello').toString('base64'),
 regex:()=>/a(b+)c/.exec('xxabbbcyy'),
 mapset:()=>{const m=new Map([[1,2]]);m.set(3,4);return m.size},
 closure:()=>(()=>{let x=1;return ()=>x++})(),
 textEncode:()=>new TextEncoder().encode('héllo'),
 inspect:()=>require('util').inspect({a:[1,{b:2}]}),
 format:()=>require('util').format('%s %d','a',1),
 errorStack:()=>new Error('x').stack,
 queueMicro:()=>queueMicrotask(()=>{}),
 promiseThen:()=>Promise.resolve(1).then(x=>x),
 structuredClone:()=>structuredClone({a:[1,2]}),
 urlSearch:()=>new URLSearchParams('a=1&b=2').toString(),
};
const which=process.argv[2]; const f=cases[which]; const out=[];
(async()=>{ for (let round=0; round<4; round++) { for (let i=0;i<30000;i++) { f(); } await new Promise(r=>setImmediate(r)); out.push(r()); }
console.log(which.padEnd(16), out.join(' ')); })();
