import assert from 'node:assert/strict';
export function exercise(api) {
  let assertions=0;
  for(const [input,expected] of [[200,'OK'],['200','OK'],['ok',200],['Not Found',404],[404,'Not Found'],[418,"I'm a Teapot"],[500,'Internal Server Error']]) {assert.equal(api(input),expected);assertions++;}
  for(const [input,error] of [[0,Error],[999,Error],['no such status',Error],[null,TypeError],[{},TypeError]]) {assert.throws(()=>api(input),error);assertions++;}
  return {package:'statuses',assertions,checks:{'':assertions},scope:'Finite numeric and message lookup plus invalid input corpus; arbitrary database entries remain obligations'};
}
