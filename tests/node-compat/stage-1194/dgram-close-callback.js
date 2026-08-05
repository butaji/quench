const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
socket.close("ignored");
