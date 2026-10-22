const a=require(process.argv[2]||'assert');
const rows=[];function check(name,fn){try{fn();rows.push([name,'ok'])}catch(e){rows.push([name,e.name,e.code,e.message])}}
check('equal',()=>a.strictEqual(1,1));check('deep',()=>a.deepStrictEqual({x:[1,2]}, {x:[1,2]}));
check('throws',()=>a.throws(()=>{throw new TypeError('boom')},/TypeError: boom/));
(async()=>{
for(const [name,expected,error] of [['regex-ok',/TypeError: boom/,new TypeError('boom')],['regex-bad',/missing/,new Error('boom')],['ctor',TypeError,new TypeError('boom')],['object',{name:'TypeError',message:/boom/},new TypeError('boom')]]){
 try{const r=await a.rejects(Promise.reject(error),expected);rows.push([name,'ok',r===undefined])}catch(e){rows.push([name,e.name,e.code,e.operator,!!e.generatedMessage])}
}
console.log(JSON.stringify(rows));
})().catch(e=>{console.error(e.stack);process.exitCode=1});
