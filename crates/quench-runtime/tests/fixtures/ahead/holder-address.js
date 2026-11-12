(function() {
            function make(delta){return function(){this.state=(this.state+delta)|0;};}
            const p={tick:make(1)},q={tick:make(2)};
            const a={next:null,state:0},b={next:a,state:0};a.next=b;
            Object.setPrototypeOf(a,p);Object.setPrototypeOf(b,q);
            const s={current:a};
            function walk(s,n){while(n>0){s.current.tick();s.current=s.current.next;n=(n-1)|0;}}
            walk(s,1000);
            if(a.state!==500||b.state!==1000)throw 'prototype identity';
            p.tick=make(4);walk(s,2);
            if(a.state!==504||b.state!==1002)throw 'method replacement';
            let reads=0;const marker={};
            Object.defineProperty(p,'tick',{get(){reads++;$262.gc();throw marker;}});
            try{walk(s,1);throw 'missing throw';}catch(e){if(e!==marker)throw e;}
            if(reads!==1||a.state!==504||s.current!==a)throw 'replayed effect';
            throw 42;
        })();
