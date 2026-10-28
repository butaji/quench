Error.prepareStackTrace=(_,frames)=>frames;
function direct(){return eval('new Error().stack')}
function nested(){return eval('eval("new Error().stack")')}
function origin(){return eval('(function escaped(){return new Error().stack})')}
function Ω(){return eval('new Error().stack')}
const inferred=function(){return eval('new Error().stack')};
const escaped=origin();
Object.defineProperty(origin,'name',{value:'changed'});
const later=origin();
let hooks=0;Object.defineProperty(origin,'name',{get(){hooks++;throw new Error('name hook')}});
const getterControl=origin();
const retained=direct();
for(let i=0;i<3;i++)gc();
const rows=[];
function record(frames,format){const f=frames[0];const value={origin:f.getEvalOrigin(),file:f.getFileName(),eval:f.isEval()};if(format)value.text=String(f);rows.push(value)}
record(direct(),true);record((0,eval)('new Error().stack'),true);record(nested(),true);
record(escaped(),true);record(later(),true);record(getterControl(),true);record(Ω(),true);record(inferred(),true);record(retained,true);
record(require('vm').runInThisContext('eval("new Error().stack")',{filename:'origin-script.js'}),true);
const Kinds=[Function,(async function(){}).constructor,(function*(){}).constructor,(async function*(){}).constructor];
(async()=>{
 for(const Kind of Kinds){const value=Kind('return new Error().stack')();record(typeof value?.next==='function'?(await value.next()).value:await value,false)}
 const inside=Function('return eval("new Error().stack")');record(inside(),false);
 console.log(JSON.stringify([rows,hooks]));
})();
