import assert from 'node:assert/strict';
export function exercise(api) {
  let assertions=0;const checks={};
  const equal=(id,actual,expected)=>{assert.deepEqual(actual,expected);assertions++;checks[id]=(checks[id]??0)+1;};
  for(const [unicode,ascii] of [['mañana.com','xn--maana-pta.com'],['bücher.example','xn--bcher-kva.example'],['EXAMPLE.COM','example.com'],['例え.テスト','xn--r8jz45g.xn--zckzah'],['ＦＯＯ。com','foo.com'],['faß.de','xn--fa-hia.de']]) {
    equal('toASCII',api.toASCII(unicode),ascii);
    const result=api.toUnicode(ascii);equal('toUnicode',result.error,false);equal('toASCII',api.toASCII(result.domain),ascii);
  }
  equal('toASCII',api.toASCII('faß.de',{transitionalProcessing:true}),'fass.de');
  equal('toUnicode',api.toUnicode('XN--BCHER-KVA.example'),{domain:'bücher.example',error:false});
  for(const [input,options] of [['-a.com',{checkHyphens:true}],['a-.com',{checkHyphens:true}],['a_b.com',{useSTD3ASCIIRules:true}],['a\u200db.com',{checkJoiners:true}],['a'.repeat(64)+'.com',{verifyDNSLength:true}],['a א.com',{checkBidi:true}]]) {
    equal('toASCII',api.toASCII(input,options),null);
    if(!options.verifyDNSLength) equal('toUnicode',api.toUnicode(input,options).error,true);
  }
  equal('toASCII',api.toASCII('a_b.com'),'a_b.com');
  equal('toASCII',api.toASCII('a'.repeat(63)+'.com',{verifyDNSLength:true}),'a'.repeat(63)+'.com');
  equal('toUnicode',api.toUnicode('xn--$'),{domain:'xn--$',error:true});
  return {package:'tr46',assertions,checks,scope:'Finite UTS46 mapping and validation corpus; Unicode versions, combinations and arbitrary inputs remain obligations'};
}
