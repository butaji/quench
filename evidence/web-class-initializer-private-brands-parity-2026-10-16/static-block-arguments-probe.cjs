function outcome(fn){try{return ['value',fn()]}catch(error){return ['throw',error.name,error.message]}}
const out=[];class C {static {out.push(outcome(()=>eval('typeof arguments')),outcome(()=>eval('(()=>typeof arguments)()')),outcome(()=>eval('(function(){return typeof arguments})()')))}}console.log(JSON.stringify(out));
