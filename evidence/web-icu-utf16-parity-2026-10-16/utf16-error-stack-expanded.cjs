
const out=[];for(const [C,args] of [[Error,[]],[TypeError,[]],[RangeError,[]],[AggregateError,[[]]],[SuppressedError,[1,2]]]){
 const error=new C(...args,'\ud800');out.push(error.message,error.toString(),error.stack.split('\n')[0]);
}
out.push(Error.prototype.toString.call({name:'\ud800',message:'\udfff'}));
for(const object of [{name:'Custom',message:'text'},{name:'',message:'text'},{},{get name(){throw 17},message:'text'}]){
 Error.captureStackTrace(object);try{out.push(object.stack.split('\n')[0])}catch(e){out.push(['threw',e])}
}
let calls=[];const target={get name(){calls.push('name');return {toString(){calls.push('name string');return '\ud800'}}},get message(){calls.push('message');return {toString(){calls.push('message string');return '\udfff'}}}};
Error.captureStackTrace(target);const first=target.stack;out.push(first.split('\n')[0],target.stack===first,calls);
Error.prepareStackTrace=(_,frames)=>frames[0].toString();function Frame(){return new Error};Object.defineProperty(Frame,'name',{value:'\ud800'});out.push(Frame().stack);
console.log(JSON.stringify(out));
