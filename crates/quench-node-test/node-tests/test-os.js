const os = require('node:os');
console.log('totalmem:', os.totalmem());
console.log('freemem:', os.freemem());
console.log('cpus:', os.cpus().length);
console.log('uptime:', os.uptime());
console.log('loadavg:', os.loadavg()[0], os.loadavg()[1], os.loadavg()[2]);
