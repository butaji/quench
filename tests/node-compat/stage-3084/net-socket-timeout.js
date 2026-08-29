"use strict";

const assert = require("assert");
const net = require("net");

const clients = [];
const server = net.createServer((client) => {
  clients.push(client);
  if (clients.length !== 2) return;
  clients[0].setTimeout(1, () => {
    clients[1].setTimeout(0);
    clients[0].end();
    clients[1].end();
  });
  clients[1].setTimeout(50);
});

server.listen(0, () => {
  let ended = 0;
  const done = () => {
    ended++;
    if (ended === 2) server.close();
  };
  for (let i = 0; i < 2; i++) {
    const client = net.connect({ port: server.address().port });
    client.on("end", done);
  }
});
