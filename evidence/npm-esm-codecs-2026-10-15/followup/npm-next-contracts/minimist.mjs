import assert from 'node:assert/strict';
export function exercise(api) {
  let assertions=0;const checks={};
  const equal=(actual,expected)=>{assert.deepEqual(actual,expected);assertions++;checks['']=(checks['']??0)+1;};
  for(const [args,options,expected] of [[['--count','3','file'],{},{_:['file'],count:3}],[['-abc'],{},{_:[],a:true,b:true,c:true}],[['--no-color'],{boolean:['color']},{_:[],color:false}],[['--count=3'],{string:['count']},{_:[],count:'3'}],[['-n','5'],{alias:{n:'number'}},{_:[],n:5,number:5}],[[],{default:{a:1}},{_:[],a:1}],[['--a','1','--a','2'],{},{_:[],a:[1,2]}],[['--nested.key','x'],{},{_:[],nested:{key:'x'}}],[['file','--a'],{stopEarly:true},{_:['file','--a']}],[['a','--','--b'],{'--':true},{_:['a'],'--':['--b']}],[['--__proto__.polluted','yes'],{},{_:[]}]])equal(api(args,options),expected);
  equal({}.polluted,undefined);
  const unknown=[];equal(api(['--skip','x','--keep','y'],{unknown:arg=>{unknown.push(arg);return arg!=='--skip';}}),{_:[],keep:'y'});equal(unknown,['--skip','--keep']);
  return {package:'minimist',assertions,checks,scope:'Finite flags, aliases, defaults, repeated/nested options, unknown callbacks and prototype-pollution corpus; arbitrary inputs remain obligations'};
}
