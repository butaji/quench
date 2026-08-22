(function(deps){'use strict';
  var assert=deps.assert, suites=[], current=null, count=0, passed=0, failures=0, skipped=0, onlyTests=[], running=false;
  function log(s){if(typeof console!=='undefined'&&console.log)console.log(s);}
  function promise(v){return v&&typeof v.then==='function'?v:Promise.resolve(v);}
  function invoke(fn,t){if(typeof fn!=='function')return undefined;if(fn.length<2)return fn(t);return new Promise(function(resolve,reject){var done=false;function finish(err){if(done)return;done=true;if(err)reject(err);else resolve();}try{fn(t,finish);}catch(e){finish(e);}});}
  function hooks(suite){var o={beforeEach:[],afterEach:[],before:[],after:[]},chain=[],i,k,j;for(;suite;suite=suite.parent)chain.unshift(suite);for(i=0;i<chain.length;i++)for(k in o)for(j=0;j<chain[i][k].length;j++)o[k].push(chain[i][k][j]);return o;}
  function run(name,opts,fn,parent){if(typeof opts==='function'){parent=fn;fn=opts;opts={};}opts=opts||{};var rec={name:String(name),opts:opts,fn:fn,parent:parent,suite:current,children:[]};if(parent)parent.children.push(rec);else test._records.push(rec);return rec;}
  function execute(rec){var h=hooks(rec.suite),ctx={assert:assert,name:rec.name,signal:{aborted:false,addEventListener:function(){}}}, children=rec.children, i, result;
    ctx.test=function(n,o,f){return run(n,o,f,rec);};
    if(rec.opts.skip||rec.opts.todo){skipped++;log('ok '+(++count)+' - '+rec.name+' # SKIP');return Promise.resolve();}
    if(test._hasOnly&&!rec.opts.only&&!hasOnlyAncestor(rec))return Promise.resolve();
    return promise().then(function(){for(i=0;i<h.before.length;i++)return promise(invoke(h.before[i],ctx)).then(function(){return executeBefore(h.before.slice(1),ctx);});}).then(function(){return executeBefore(h.beforeEach,ctx);}).then(function(){return invoke(rec.fn,ctx);}).then(function(v){result=v;return promise(result);}).then(function(){return runChildren(children);}).then(function(){return executeAfter(h.afterEach.slice().reverse(),ctx);}).then(function(){return executeAfter(h.after.slice().reverse(),ctx);}).then(function(){passed++;log('ok '+(++count)+' - '+rec.name);},function(e){failures++;log('not ok '+(++count)+' - '+rec.name+' '+(e&&e.message||e));});
  }
  function hasOnlyAncestor(rec){for(var p=rec;p;p=p.parent)if(p.opts&&p.opts.only)return true;return false;}
  function executeBefore(a,t){var i=0;function n(){return i<a.length?promise(invoke(a[i++],t)).then(n):Promise.resolve();}return n();}
  function executeAfter(a,t){var i=0;function n(){return i<a.length?promise(invoke(a[i++],t)).then(n):Promise.resolve();}return n();}
  function runChildren(a){var i=0;function n(){return i<a.length?execute(a[i++]).then(n):Promise.resolve();}return n();}
  function test(name,opts,fn){return run(name,opts,fn,null);} test._records=[]; test._hasOnly=false;
  test.skip=function(n,o,f){return test(n,typeof o==='function'?{skip:true}:Object.assign({},o,{skip:true}),typeof o==='function'?o:f);};
  test.todo=function(n,o,f){return test(n,typeof o==='function'?{todo:true}:Object.assign({},o,{todo:true}),typeof o==='function'?o:f);};
  test.only=function(n,o,f){test._hasOnly=true;return test(n,typeof o==='function'?{only:true}:Object.assign({},o,{only:true}),typeof o==='function'?o:f);};
  test.run=function(){running=true;var a=test._records.slice(),i=0;function next(){return i<a.length?execute(a[i++]).then(next):Promise.resolve();}return next().then(function(){running=false;return test.summary();});};
  function describe(name,opts,fn){if(typeof opts==='function'){fn=opts;opts={};}opts=opts||{};var s={name:String(name),options:opts,beforeEach:[],afterEach:[],before:[],after:[],parent:current},old=current;suites.push(s);current=s;try{invoke(fn,{test:test,assert:assert});}catch(e){failures++;}suites.pop();current=old;}
  function hook(k,fn){if(current&&typeof fn==='function')current[k].push(fn);} describe.beforeEach=function(f){hook('beforeEach',f);};describe.afterEach=function(f){hook('afterEach',f);};describe.before=function(f){hook('before',f);};describe.after=function(f){hook('after',f);};
  test.test=test;test.describe=describe;test.suite=describe;test.it=test;test.beforeEach=describe.beforeEach;test.afterEach=describe.afterEach;test.before=describe.before;test.after=describe.after;test.summary=function(){return {tests:count,pass:passed,fail:failures,skip:skipped,passed:passed,failed:failures,skipped:skipped};};test.assert=assert;
  return test;
})