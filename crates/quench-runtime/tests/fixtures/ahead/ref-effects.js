(function() {
            function replace(s,value,n){while(n>0){s.current=value;n=(n-1)|0;}}
            const s={current:{id:0}};
            replace(s,s.current,10000);
            $262.gc();
            let young={id:42};
            replace(s,young,64);
            young=null;
            $262.gc();
            if(s.current.id!==42)throw 'collected reference';
            replace(s,-0,64);
            if(!Object.is(s.current,-0))throw 'widening';
            replace(s,null,64);
            if(s.current!==null)throw 'null';
            const marker={};let calls=0;
            Object.defineProperty(s,'current',{set(value){calls++;if(value!==marker)throw 'argument';throw marker;}});
            try{replace(s,marker,1);throw 'missing throw';}catch(e){if(e!==marker)throw e;}
            if(calls!==1)throw 'replayed setter';
            throw 42;
        })();
