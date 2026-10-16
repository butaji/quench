Error.prepareStackTrace=(_,frames)=>frames;
function Loose(){return new Error}
function Strict(){'use strict';return Loose()}
function StrictLeaf(){'use strict';return new Error}
function Outer(){return StrictLeaf()}
for(const [invoke,expected] of [[Loose,[Loose]],[Strict,[Loose,Strict]],[Outer,[StrictLeaf,Outer]]]){
 const sites=invoke().stack;console.log(JSON.stringify(sites.slice(0,expected.length).map((s,i)=>[s.getFunction()===expected[i],s.getFunction()===undefined,s.getThis()===undefined])));
}
