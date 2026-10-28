import assert from 'node:assert/strict';
export function exercise(api) {
  let assertions=0;const checks={};
  const equal=(id,actual,expected)=>{assert.deepEqual(actual,expected);assertions++;checks[id]=(checks[id]??0)+1;};
  const throws=(id,fn,error)=>{assert.throws(fn,error);assertions++;checks[id]=(checks[id]??0)+1;};
  for(const [text,expected] of [["// comment\n{unquoted:'x',hex:0xff,trailing:[1,2,],}",{unquoted:'x',hex:255,trailing:[1,2]}],['[+1,-.5,5.,1e2]',[1,-.5,5,100]],['[Infinity,-Infinity,NaN]',[Infinity,-Infinity,NaN]],['true',true],['null',null],["'a\\\nb'",'ab']]) equal('parse',api.parse(text),expected);
  const visits=[];
  equal('parse',api.parse('{a:1,b:2}',function(key,value){visits.push([key,this===value]);return key==='b'?undefined:typeof value==='number'?value*2:value;}),{a:2});
  equal('parse',visits.map(row=>row[0]),['a','b','']);
  for(const invalid of ['{a:}', '[1,,2]', '{a:1', 'undefined', '1 2']) throws('parse',()=>api.parse(invalid),SyntaxError);
  for(const [value,expected] of [[{a:1,b:[true,null,'x']},"{a:1,b:[true,null,'x']}"],[{'a b':'x'},"{'a b':'x'}"],[[Infinity,-Infinity,NaN],'[Infinity,-Infinity,NaN]'],[{a:undefined,b:()=>0},'{}'],[[undefined,()=>0],'[null,null]']]) equal('stringify',api.stringify(value),expected);
  equal('stringify',api.stringify({a:1,b:2},['b']),'{b:2}');
  equal('stringify',api.stringify({a:1},(key,value)=>typeof value==='number'?value+1:value),'{a:2}');
  equal('stringify',api.stringify({a:'x'},{quote:'"'}),'{a:"x"}');
  equal('stringify',api.stringify(undefined),undefined);
  const circular={};circular.self=circular;throws('stringify',()=>api.stringify(circular),TypeError);
  return {package:'json5',assertions,checks,scope:'Finite parsing, errors, reviver, serializer and replacer corpus; arbitrary inputs remain obligations'};
}
