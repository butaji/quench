import assert from 'node:assert/strict';
export function exercise(api) {
  let assertions=0;const checks={};
  const equal=(id,actual,expected)=>{assert.deepEqual(actual,expected);assertions++;checks[id]=(checks[id]??0)+1;};
  const throws=(id,fn,error)=>{assert.throws(fn,error);assertions++;checks[id]=(checks[id]??0)+1;};
  for(const [unicode,encoded] of [['',''],['abc','abc-'],['mañana','maana-pta'],['bücher','bcher-kva'],['café','caf-dma'],['☃','n3h'],['é','9ca']]) {
    equal('encode',api.encode(unicode),encoded);equal('decode',api.decode(encoded),unicode);
  }
  for(const [unicode,ascii] of [['mañana.com','xn--maana-pta.com'],['bücher.example','xn--bcher-kva.example'],['a@mañana.com','a@xn--maana-pta.com'],['example.com','example.com'],['mañana。com','xn--maana-pta.com'],['mañana.com.','xn--maana-pta.com.']]) {
    equal('toASCII',api.toASCII(unicode),ascii);equal('toUnicode',api.toUnicode(ascii),unicode.replace('。','.'));
  }
  equal('toUnicode',api.toUnicode('XN--MAANA-PTA.com'),'XN--MAANA-PTA.com');
  for(const [text,points] of [['',[]],['abc',[97,98,99]],['𝌆',[0x1d306]],['\ud800x\udc00',[0xd800,120,0xdc00]],['a😀é',[97,0x1f600,233]]]) {
    equal('ucs2.decode',api.ucs2.decode(text),points);equal('ucs2.encode',api.ucs2.encode(points),text);
  }
  throws('decode',()=>api.decode('$'),RangeError);
  throws('ucs2.encode',()=>api.ucs2.encode([0x110000]),RangeError);
  return {package:'punycode',assertions,checks,scope:'Finite public Punycode and UCS-2 corpus, including malformed input and domain separators; arbitrary inputs remain obligations'};
}
