import assert from 'node:assert/strict';
export function exercise(api) {
  let assertions=0;const checks={};
  const equal=(id,actual,expected)=>{assert.deepEqual(actual,expected);assertions++;checks[id]=(checks[id]??0)+1;};
  for(const [text,options,expected] of [['a=1&a=2',{}, {a:['1','2']}],['a[b]=x&a[c][]=y&a[c][]=z',{}, {a:{b:'x',c:['y','z']}}],['?x=a+b',{ignoreQueryPrefix:true},{x:'a b'}],['a;b=2',{delimiter:';',strictNullHandling:true},{a:null,b:'2'}],['a=1,2',{comma:true},{a:['1','2']}],['a.b=c',{allowDots:true},{a:{b:'c'}}],['a[0]=x&a[2]=z',{}, {a:['x','z']}],['a=1&a=2',{duplicates:'first'},{a:'1'}],['a=1&a=2',{duplicates:'last'},{a:'2'}],['__proto__[polluted]=yes',{},{}],['',{},{}]]) equal('parse',api.parse(text,options),expected);
  for(const [value,options,expected] of [[{a:'x y',b:'é'}, {},'a=x%20y&b=%C3%A9'],[{a:[1,2]},{arrayFormat:'indices'},'a%5B0%5D=1&a%5B1%5D=2'],[{a:[1,2]},{arrayFormat:'repeat'},'a=1&a=2'],[{a:[1,2]},{arrayFormat:'comma'},'a=1%2C2'],[{a:{b:'c'}},{allowDots:true},'a.b=c'],[{a:null,b:undefined},{strictNullHandling:true},'a'],[{a:null,b:'x'},{skipNulls:true},'b=x'],[{b:2,a:1},{sort:(a,b)=>a.localeCompare(b)},'a=1&b=2'],[{a:'x y'},{format:'RFC1738'},'a=x+y'],[{}, {},'']]) equal('stringify',api.stringify(value,options),expected);
  equal('formats.formatters.RFC1738',api.formats.formatters.RFC1738('a%20b%20c'),'a+b+c');
  equal('formats.formatters.RFC3986',api.formats.formatters.RFC3986('a%20b'),'a%20b');
  const calls=[];
  equal('parse',api.parse('a=1',{decoder(value,defaultDecoder,charset,type){calls.push(type);return defaultDecoder(value,defaultDecoder,charset,type);}}),{a:'1'});
  equal('parse',calls,['key','value']);
  return {package:'qs',assertions,checks,scope:'Finite nested query, array, duplicate, codec, formatter and pollution corpus; arbitrary inputs remain obligations'};
}
