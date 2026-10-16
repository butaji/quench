const cases=[undefined,0,7,'7','0',NaN,Infinity,-1,4294967296,true];let index=0;
function next(){if(index===cases.length)return;const backlog=cases[index++],server=require('net').createServer();server.listen({port:0,host:'127.0.0.1',backlog},()=>server.close(next))}next();
