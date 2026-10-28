const {inspect}=require('node:util'),{getConstructorName}=require('internal/test/binding').internalBinding('util');
class X{}const direct=new X,created=Object.create(X.prototype);X.prototype.constructor=null;
const before=[getConstructorName(direct),getConstructorName(created),inspect(direct),inspect(created)];
Object.setPrototypeOf(direct,null);before.push(getConstructorName(direct),inspect(direct));console.log(JSON.stringify(before));
