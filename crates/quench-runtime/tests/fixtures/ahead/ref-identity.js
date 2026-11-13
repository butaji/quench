(function() {
                function walk(s,n){while(n>0){s.current=s.current.next;n=(n-1)|0;}}
                const a={next:null},b={next:a};a.next=b;
                const s={current:a};
                walk(s,10001);
                if(s.current!==b || a.next!==b || b.next!==a)throw 'identity';
                throw 42;
            })();
