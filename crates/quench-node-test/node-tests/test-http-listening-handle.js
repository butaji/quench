const http=require('http'),server=http.createServer(),events=[];
function snapshot(){return [server.listening,server.address()===null,server._handle===null,Object.hasOwn(server,'listening')]}
const initial=snapshot();server.on('listening',()=>events.push('listening'));server.on('close',()=>{events.push('close');console.log(JSON.stringify([initial,returned,closed,events]))});server.listen(0);const returned=snapshot();server.close();const closed=snapshot();
