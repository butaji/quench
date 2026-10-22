(function(){var N=2000000;var o={x:0}; for(var i=0;i<N;i++) o.x=i;if(N ? o.x!==N-1 : o.x!==0) throw new Error('field-write check');})();
