for(const path of ['https://example.org/','file://remote/'])try{require('node:url').fileURLToPath(path)}catch(e){console.log(String(e),JSON.stringify(Reflect.ownKeys(e).filter(k=>k!=='stack')))}
