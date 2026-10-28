import assert from 'node:assert/strict';
export function exercise(api) {
  let assertions=0;const checks={};
  const equal=(actual,expected)=>{assert.equal(actual,expected);assertions++;checks['']=(checks['']??0)+1;};
  for(const [text,width] of [['',0],['abc',3],['古',2],['a古b',4],['e\u0301',1],['\u0301',0],['😀',2],['👩‍👩‍👧‍👦',2],['🇬🇧',2],['\u001b[31mred\u001b[39m',3],['a\tb\nc',3],['\ud800',0],['가',2],['1\ufe0f\u20e3',2]]) equal(api(text),width);
  equal(api('·',{ambiguousIsNarrow:true}),1);equal(api('·',{ambiguousIsNarrow:false}),2);
  return {package:'string-width',assertions,checks,scope:'Finite terminal width corpus for ANSI, combining, CJK, emoji and Hangul; arbitrary Unicode and options remain obligations'};
}
