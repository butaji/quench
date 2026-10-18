const dns=require('dns');const crypto=require('crypto');
process.on('exit',()=>{
 dns.lookup('localhost',()=>console.log('UNEXPECTED dns callback'));
 crypto.generateKey('hmac',{length:128},()=>console.log('UNEXPECTED crypto callback'));
 console.log('native-exit-work-accepted');
});
