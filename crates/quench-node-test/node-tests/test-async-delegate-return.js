'use strict';
(async()=>{
 for(const mode of ['sync','async','missing']){
  const events=[];
  const inner=mode==='missing'?{[Symbol.iterator](){return this},next(){return{done:false,value:1}}}:mode==='sync'?(function*(){try{yield 1}finally{events.push('inner-finally')}})():(async function*(){try{yield 1}finally{events.push('inner-finally')}})();
  async function* outer(){try{yield* inner}finally{events.push('outer-finally')}}
  const g=outer();await g.next();console.log(mode,JSON.stringify(await g.return('return')),JSON.stringify(events));
 }
})().catch(e=>{console.error(e);process.exitCode=1});
