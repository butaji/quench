(function(deps){'use strict';
  var assert = deps.assert, suites = [], current = null, failures = 0, count = 0;
  function log(s){ if (typeof console !== 'undefined' && console.log) console.log(s); }
  function invoke(fn,t){ if(typeof fn==='function') return fn(t); }
  function hooks(){ var o={beforeEach:[],afterEach:[],before:[],after:[]}; for(var i=0;i<suites.length;i++) for(var k in o) for(var j=0;j<suites[i][k].length;j++) o[k].push(suites[i][k][j]); return o; }
  function run(name, opts, fn){ if(typeof opts==='function'){fn=opts;opts={};} opts=opts||{}; count++; if(opts.skip){log('ok '+count+' - '+name+' # SKIP');return;} var h=hooks(),t={assert:assert}; t.test=test; try{for(var i=0;i<h.beforeEach.length;i++)invoke(h.beforeEach[i],t);for(var i=0;i<h.before.length;i++)invoke(h.before[i],t);invoke(fn,t);for(var i=h.afterEach.length-1;i>=0;i--)invoke(h.afterEach[i],t);for(var i=h.after.length-1;i>=0;i--)invoke(h.after[i],t);log('ok '+count+' - '+name);}catch(e){failures++;log('not ok '+count+' - '+name);}}
  function test(name,opts,fn){run(String(name),opts,fn);} test.skip=function(name,fn){run(String(name),{skip:true},fn);};
  function describe(name,fn){var s={beforeEach:[],afterEach:[],before:[],after:[]},old=current;suites.push(s);current=s;try{invoke(fn,{test:test,assert:assert});}catch(e){failures++;log('not ok - '+name);}suites.pop();current=old;}
  function hook(k,fn){if(current&&typeof fn==='function')current[k].push(fn);} describe.beforeEach=function(f){hook('beforeEach',f);};describe.afterEach=function(f){hook('afterEach',f);};describe.before=function(f){hook('before',f);};describe.after=function(f){hook('after',f);};
  test.test=test;test.describe=describe;test.it=test;test.beforeEach=describe.beforeEach;test.afterEach=describe.afterEach;test.before=describe.before;test.after=describe.after;test.summary=function(){log('tests '+count+', failures '+failures);};test.assert=assert;
  return test;
})
