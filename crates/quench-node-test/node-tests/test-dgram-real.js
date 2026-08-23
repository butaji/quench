const dgram = require('node:dgram');
const receiver = dgram.createSocket('udp4');
receiver.bind(34123, '127.0.0.1');
const address = receiver.address();
if (!address || address.port !== 34123) throw new Error('dgram address');
const sender = dgram.createSocket('udp4');
sender.bind(34124, '127.0.0.1');
if (typeof sender.setTTL !== 'function' || typeof sender.setBroadcast !== 'function' ||
    typeof sender.setMulticastTTL !== 'function' || typeof sender.setMulticastLoopback !== 'function' ||
    typeof sender.addMembership !== 'function' || typeof sender.dropMembership !== 'function') throw new Error('dgram methods');
sender.setTTL(64);
sender.setBroadcast(true);
sender.setMulticastTTL(64);
sender.setMulticastLoopback(true);
sender.addMembership('239.255.0.1');
sender.dropMembership('239.255.0.1');
if (typeof sender.getSendQueueCount() !== 'number') throw new Error('dgram queue');
sender.send('ping', 34123, '127.0.0.1');
sender.close();
receiver.close();
console.log('dgram-real: ok');