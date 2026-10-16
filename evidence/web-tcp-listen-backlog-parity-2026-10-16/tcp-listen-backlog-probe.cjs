const net=require('net');const server=net.createServer();server.listen(0,'127.0.0.1',()=>{console.log(server.address().port>0);server.close()});
