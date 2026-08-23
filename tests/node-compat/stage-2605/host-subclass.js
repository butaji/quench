const E=require('events'); class X extends E { foo(){return 7;} } const x=new X(); console.log(typeof x.foo,x.foo());
