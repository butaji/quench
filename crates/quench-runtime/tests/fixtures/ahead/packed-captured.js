(function(){
    let mask=4;
    function Node(link,state){this.link=link;this.state=state;}
    Node.prototype.mark=function(){'use strict';this.state=this.state|mask;};
    function Owner(node){this.count=0;this.current=node;}
    Owner.prototype.advance=function(){this.count++;this.current.mark();return this.current.link;};
    function walk(owner,count){for(let i=0;i<count;i=(i+1)|0)owner.current=owner.advance();}
    let a=new Node(null,0),b=new Node(a,1);a.link=b;
    let owner=new Owner(a);walk(owner,1000);
    if(owner.current!==a||owner.count!==1000||a.state!==4||b.state!==5)throw 'state';
    throw 42;
})();
