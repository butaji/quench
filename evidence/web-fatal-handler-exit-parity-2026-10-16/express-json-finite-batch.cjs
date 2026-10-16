'use strict';
const http=require('http');
const body=JSON.stringify({hello:'world'});
const count=500;
const agent=new http.Agent({keepAlive:true,maxSockets:count});
let completed=0;
const errors=[];
const start=Date.now();
function finish(error){if(error)errors.push(String(error));if(++completed===count){agent.destroy();console.log(JSON.stringify({requested:count,completed,errors,elapsed_ms:Date.now()-start}));}}
for(let index=0;index<count;index++){
 let settled=false;
 function settle(error){if(!settled){settled=true;finish(error);}}
 const request=http.request({hostname:'127.0.0.1',port:3000,path:'/json',method:'POST',agent,headers:{'Content-Type':'application/json','Content-Length':Buffer.byteLength(body)}},response=>{
  let data='';response.setEncoding('utf8');response.on('data',chunk=>data+=chunk);
  response.on('error',settle);response.on('end',()=>{try{if(response.statusCode!==200)throw new Error('status '+response.statusCode);const parsed=JSON.parse(data);if(parsed.hello!=='world'||Object.keys(parsed).length!==1)throw new Error('body '+data);settle();}catch(error){settle(error);}});
 });
 request.setTimeout(30000,()=>request.destroy(new Error('request timeout')));request.on('error',settle);request.end(body);
}
