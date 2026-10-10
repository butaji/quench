(function(){var N=0;function fn(x){return x+1;} var r=0; for(var i=0;i<N;i++) r=fn(i);if(N ? r!==N : r!==0) throw new Error('local-call check');})();
