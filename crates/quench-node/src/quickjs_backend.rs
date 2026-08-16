use crate::{BOOTSTRAP_SOURCE, MKDTEMP_SEQUENCE};
use hmac::{Hmac, Mac};
use md5::Md5;
use rand::RngCore;
use rquickjs::{function::Func, Context, Runtime};
use sha1::Sha1;
use sha2::{Digest, Sha224, Sha256, Sha384, Sha512};
use sha3::{digest::ExtendableOutput, digest::Update, Shake128, Shake256};
use std::{
    env, fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) fn execute_source(
    source: &str,
    runtime: &Runtime,
    path: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    runtime.set_loader(crate::esm::NodeResolver, crate::esm::NodeLoader);
    let context = Context::full(runtime)?;
    if let Some(path) = path {
        let filename = if path.is_absolute() {
            path.to_path_buf()
        } else {
            env::current_dir()?.join(path)
        };
        let dirname = filename.parent().unwrap_or_else(|| Path::new("."));
        context.with(|ctx| -> rquickjs::Result<()> {
            ctx.globals()
                .set("__filename", filename.to_string_lossy().as_ref())?;
            ctx.globals()
                .set("__dirname", dirname.to_string_lossy().as_ref())?;
            ctx.globals().set(
                "__quench_script_filename",
                filename.to_string_lossy().as_ref(),
            )?;
            let script_name = filename.to_string_lossy().to_string();
            let mut script_args = Vec::new();
            let mut found_script = false;
            for argument in env::args().skip(1) {
                if found_script {
                    if argument == "--" {
                        continue;
                    }
                    script_args.push(argument);
                } else if argument == script_name || argument == path.to_string_lossy() {
                    found_script = true;
                }
            }
            ctx.globals().set("__quench_script_args", script_args)?;
            Ok(())
        })?;
    }
    run_host_context!(context, source)?;
    Ok(())
}
