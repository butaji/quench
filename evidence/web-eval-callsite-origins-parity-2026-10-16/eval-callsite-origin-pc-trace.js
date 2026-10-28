Error.prepareStackTrace=(_,frames)=>frames;function direct(){return eval('new Error().stack')}console.log(direct()[0].getEvalOrigin());
