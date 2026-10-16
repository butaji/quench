import assert from 'node:assert/strict';
export function exercise(api) {
  let assertions=0;const checks={};
  const equal=(id,actual,expected)=>{assert.deepEqual(actual,expected);assertions++;checks[id]=(checks[id]??0)+1;};
  for(const [value,text,expected=value] of [[Buffer.from([0,1,255]),'":base64:AAH/"'],[':base64:not-buffer','"::base64:not-buffer"'],[':x','"::x"'],[{a:Buffer.from('hello'),b:[1,null]},'{"a":":base64:aGVsbG8=","b":[1,null]}'],[{a:undefined,b:true},'{"b":true}',{b:true}],[[undefined,2],'[null,2]',[null,2]],[null,'null']]) {
    equal('stringify',api.stringify(value),text);
    equal('parse',api.parse(text),expected);
  }
  equal('stringify',api.stringify(undefined),undefined);
  let error;try{api.parse('{invalid');}catch(value){error=value;}equal('parse',error instanceof SyntaxError,true);
  return {package:'json-buffer',assertions,checks,scope:'Finite binary JSON, escaped marker, nested value, omission and malformed input corpus; arbitrary inputs remain obligations'};
}
