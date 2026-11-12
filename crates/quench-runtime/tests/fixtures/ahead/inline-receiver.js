function fold(n,x){const state={count:n|0,control:x|0};while(state.count>0)advance(state);return state.control|0;}
function advance(state){state.count=(state.count-1)|0;state.control=(state.control>>1)^((state.control&1)?7:0);}
if(fold(0,-1)!==-1||fold(-1,17)!==17||fold(1,-1)!==-8||fold(3,3)!==6)throw 'inline receiver';
throw 42;
