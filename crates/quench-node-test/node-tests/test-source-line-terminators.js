const out=[],vm=require('vm');Error.prepareStackTrace=(_,frames)=>frames.filter(site=>site.getFileName()==='line-break.js').map(site=>[site.getLineNumber(),site.getColumnNumber()]);
for(const separator of ['\r','\r\n','\u2028','\u2029']){
 const source='(function(){'+separator+'const Capture=()=>new Error().stack;'+separator+"const hint='😀';return Capture()"+separator+'})()';
 out.push(vm.runInThisContext(source,{filename:'line-break.js'}));
}
console.log(JSON.stringify(out));
