console.log(JSON.stringify(['crypto/util','per_context/primordials','bootstrap/realm'].map(name=>require('./internal/'+name+'.js'))));
