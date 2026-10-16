const results=[];
for(const index of [0,100000]){
 const array=[];let reads=0;const getter=()=>{reads++;return 7};
 Object.defineProperty(array,index,{get:getter,configurable:true,enumerable:true});Object.preventExtensions(array);
 try{Object.defineProperty(array,index,{value:42,writable:true});Object.freeze(array);results.push([index,array[index],reads,Reflect.defineProperty(array,index,{value:42}),Reflect.defineProperty(array,index,{value:43})]);}
 catch(error){results.push([index,error.name,error.message]);}
}
console.log(JSON.stringify({complete:true,results}));
