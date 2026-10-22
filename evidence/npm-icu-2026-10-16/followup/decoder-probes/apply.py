from pathlib import Path
import sys,shutil
root=Path(sys.argv[1]);here=Path(__file__).parent
changes={}
def read(name):
 if name not in changes:changes[name]=(root/name).read_text()
 return changes[name]
def replace(name,old,new,count=1):
 s=read(name);assert s.count(old)==count,(name,old[:80],s.count(old));changes[name]=s.replace(old,new)
module='crates/quench-node/src/modules/'
replace(module+'mod.rs','pub(crate) mod text_decoder_icu;','pub(crate) mod text_decoder_icu;\n#[cfg(quench_icu)]\npub(crate) mod icu_binding_shared_vm;')
replace('crates/quench-runtime/src/api_native.rs','($( $scope:ident $name:literal ($length:literal) => $callback:path ),* $(,)?) => {\n        &[$($crate::HostFunction {','($( $(#[$attribute:meta])* $scope:ident $name:literal ($length:literal) => $callback:path ),* $(,)?) => {\n        &[$($(#[$attribute])* $crate::HostFunction {')
replace('crates/quench-node/src/host/shared_vm.rs','        method "zlibStreamOpen"', ''.join('        #[cfg(quench_icu)]\n        method "'+name+'" ('+str(n)+') => crate::modules::icu_binding_shared_vm::'+f+',\n' for name,n,f in [('icuGetConverter',2,'get_converter'),('icuDecode',4,'decode'),('icuStringWidth',3,'string_width'),('icuTranscode',3,'transcode'),('icuErrorName',1,'error_name')])+'        method "zlibStreamOpen"')
replace(module+'trace_shared_vm.rs','        "encoding_binding" => return super::encoding_binding_shared_vm::binding(context),','''        "encoding_binding" => return super::encoding_binding_shared_vm::binding(context),
        #[cfg(quench_icu)]
        "icu" => return super::icu_binding_shared_vm::binding(context),
        "config" => {
            let has_intl = context.boolean(cfg!(quench_icu));
            put(context, binding, "hasIntl", has_intl)?;
        }''')
state='crates/quench-node/src/host/node_host.rs'
replace(state,'    pub(crate) tls_contexts: crate::modules::tls_shared_vm::contexts::State,','    pub(crate) tls_contexts: crate::modules::tls_shared_vm::contexts::State,\n    #[cfg(quench_icu)]\n    pub(crate) icu_converters: crate::modules::icu_binding_shared_vm::State,')
replace(state,'        self.tls_contexts = crate::modules::tls_shared_vm::contexts::State::default();','        self.tls_contexts = crate::modules::tls_shared_vm::contexts::State::default();\n        #[cfg(quench_icu)]\n        { self.icu_converters = crate::modules::icu_binding_shared_vm::State::default(); }')
replace(state,'            tls_contexts: crate::modules::tls_shared_vm::contexts::State::default(),','            tls_contexts: crate::modules::tls_shared_vm::contexts::State::default(),\n            #[cfg(quench_icu)]\n            icu_converters: crate::modules::icu_binding_shared_vm::State::default(),')
process=module+'process_shared_vm.rs'
s=read(process);anchor='super::tls_shared_vm::contexts::cleanup(runtime);';assert s.count(anchor)==2
s=s.replace(anchor,anchor+'\n        #[cfg(quench_icu)]\n        super::icu_binding_shared_vm::cleanup(runtime);');changes[process]=s
replace(process,'            super::tls_shared_vm::contexts::prune(runtime);','            super::tls_shared_vm::contexts::prune(runtime);\n            #[cfg(quench_icu)]\n            super::icu_binding_shared_vm::prune(runtime);')
replace('crates/quench-node/src/host/shared_vm/commonjs.rs','fn node_library_module(context:','pub(crate) fn node_library_module(context:')
text=module+'text_decoder_shared_vm.rs'
replace(text,'const FACTORY: &str','#[cfg(not(quench_icu))]\nconst FACTORY: &str')
s=read(text);a=s.index('    let canonical = context.host_function',s.index('pub(crate) fn initialize('));b=s.index('    let decoder_key =',a);old=s[a:b]
new='''    #[cfg(quench_icu)]
    let constructors = crate::host::shared_vm::commonjs::node_library_module(
        context, "util.js", "internal/encoding.js"
    )?;
    #[cfg(not(quench_icu))]
    let constructors = {
'''+''.join('    '+line+'\n' for line in old.splitlines() if line.strip()!='let undefined = context.undefined();')+'''        constructors
    };
    let undefined = context.undefined();
'''
# Legacy factory calls also need their local undefined value.
new=new.replace('        let constructors = context.call_rooted(factory, undefined,','        let undefined = context.undefined();\n        let constructors = context.call_rooted(factory, undefined,')
changes[text]=s[:a]+new+s[b:]
# Mutate only after every source anchor validated.
for name,s in changes.items():(root/name).write_text(s)
for name in ['text_decoder_icu.rs','icu_binding_shared_vm.rs']:shutil.copy2(here/name,root/module/name)
print('\n'.join([*changes,module+'text_decoder_icu.rs',module+'icu_binding_shared_vm.rs']))
