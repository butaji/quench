import assert from 'node:assert/strict';
import corpus from './color-convert-cases.json' with {type:'json'};

export function exercise(api) {
  let assertions=0;const checks={};
  const view=new DataView(new ArrayBuffer(8));
  const encode=value=>{
    if(typeof value==='number'){
      view.setFloat64(0,value);
      return {numberBits:view.getBigUint64(0).toString(16).padStart(16,'0')};
    }
    if(Array.isArray(value))return value.map(encode);
    if(value===undefined)return {undefined:true};
    return value;
  };
  const equal=(id,actual,expected)=>{
    assert.deepEqual(actual,expected);assertions++;checks[id]=(checks[id]??0)+1;
  };
  const resolve=id=>id.split('.').reduce((value,key)=>value[key],api);
  const ids=[...new Set(corpus.rows.map(row=>row.id))].sort();
  const actualIds=[];
  for(const from of Object.keys(api))for(const to of Object.keys(api[from])){
    if(typeof api[from][to]!=='function')continue;
    actualIds.push(`${from}.${to}`);
    if(typeof api[from][to].raw==='function')actualIds.push(`${from}.${to}.raw`);
  }
  assert.deepEqual(actualIds.sort(),ids);assertions++;
  assert.equal(corpus.complete,true);assert.equal(corpus.functions,544);assert.equal(corpus.cases,2176);assertions+=3;
  for(const {id,input,variadic,result}of corpus.rows){
    const fn=resolve(id);
    equal(id,encode(variadic?fn(...input):fn(input)),result);
  }
  for(const id of ids){
    const fn=resolve(id);
    equal(id,fn(null),null);equal(id,fn(undefined),undefined);
  }
  return {package:'color-convert',assertions,checks,scope:'All discovered rounded/raw conversion functions on pinned finite model samples, exact binary64 outputs, array/variadic inputs and nullish short circuit; arbitrary inputs and future exports remain obligations'};
}
