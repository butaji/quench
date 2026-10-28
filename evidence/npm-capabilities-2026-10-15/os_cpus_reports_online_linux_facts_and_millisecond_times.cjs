
const assert=require('node:assert'),fs=require('node:fs'),os=require('node:os');
function snapshot(){return fs.readFileSync('/proc/stat','utf8').split('\n').filter(s=>/^cpu\d+ /.test(s)).map(s=>s.trim().split(/\s+/).slice(1).map(Number))}
const before=snapshot(),cpus=os.cpus(),after=snapshot();
assert.strictEqual(os.cpus.name,'cpus');assert.strictEqual(os.cpus.length,0);
assert.strictEqual(cpus.length,before.length);assert(Array.isArray(new os.cpus()));
for(let i=0;i<cpus.length;i++){
 const cpu=cpus[i];assert.deepStrictEqual(Object.keys(cpu),['model','speed','times']);
 assert.strictEqual(typeof cpu.model,'string');assert(Number.isInteger(cpu.speed)&&cpu.speed>=0);
 const keys=['user','nice','sys','idle','irq'],indices=[0,1,2,3,5];
 assert.deepStrictEqual(Object.keys(cpu.times),keys);
 for(let j=0;j<keys.length;j++){const value=cpu.times[keys[j]],index=indices[j];assert(Number.isInteger(value));assert(value>=before[i][index]*10&&value<=after[i][index]*10)}
}

console.log(JSON.stringify([cpus.length,cpus.map(cpu=>cpu.model)]));
