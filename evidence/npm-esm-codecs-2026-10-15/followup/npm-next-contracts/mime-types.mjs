import assert from 'node:assert/strict';
export function exercise(api) {
  let assertions=0;const checks={};
  const equal=(id,actual,expected)=>{assert.deepEqual(actual,expected);assertions++;checks[id]=(checks[id]??0)+1;};
  for(const [input,expected] of [['file.html','text/html'],['/a/b.JSON','application/json'],['.txt','text/plain'],['unknown-extension.zzzz',false],['',false],[null,false]]) equal('lookup',api.lookup(input),expected);
  for(const [input,expected] of [['text/html','UTF-8'],['application/json','UTF-8'],['text/plain; charset=ascii','UTF-8'],['image/png',false],[null,false]]) {
    equal('charset',api.charset(input),expected);equal('charsets.lookup',api.charsets.lookup(input),expected);
  }
  for(const [input,expected] of [['text/html','html'],['application/json','json'],['TEXT/HTML; charset=UTF-8','html'],['bad/type',false],[null,false]]) equal('extension',api.extension(input),expected);
  for(const [input,expected] of [['html','text/html; charset=utf-8'],['json','application/json; charset=utf-8'],['image/png','image/png'],['text/html; charset=iso-8859-1','text/html; charset=iso-8859-1'],['unknown.zzzz',false],[null,false]]) equal('contentType',api.contentType(input),expected);
  return {package:'mime-types',assertions,checks,scope:'Finite MIME extension, lookup, charset and content-type corpus; database completeness and arbitrary inputs remain obligations'};
}
