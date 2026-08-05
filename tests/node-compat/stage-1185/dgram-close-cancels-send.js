const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
socket.bind();
socket.send(Buffer.from("pending"), 40000, "127.0.0.1");
socket.close();
