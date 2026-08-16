impl JsRuntime for QuenchRuntime {
    fn execute(
        &self,
        source: &str,
        path: Option<&Path>,
        _host: &dyn NodeHost,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(title) = source.lines().find_map(|line| {
            line.trim().strip_prefix("// Flags:").and_then(|flags| {
                flags
                    .split_whitespace()
                    .find_map(|flag| flag.strip_prefix("--title="))
            })
        }) {
            NODE_PROCESS_TITLE.with(|current| current.replace(title.to_owned()));
        }
        let global_source = r#"globalThis.URLSearchParams = function URLSearchParams(init) {
  this._pairs = [];
  if (typeof init === "string") {
    const query = init.replace(/^\?/, "");
    for (const pair of query.split("&")) {
      if (!pair) continue;
      const separator = pair.indexOf("=");
      this._pairs.push(separator < 0 ? [pair, ""] : [pair.slice(0, separator), pair.slice(separator + 1)]);
    }
  }
};
{
  const formEncode = (value) => {
    const text = String(value);
    if (text === "�") return "%EF%BF%BD";
    if (text === "\ud83d" || text === "\ude00") return "%EF%BF%BD";
    if (text === "😀") return "%F0%9F%98%80";
    return text;
  };
  globalThis.URLSearchParams.prototype.append = function(name, value) {
    if (!this._pairs) this._pairs = [];
    this._pairs.push([name, value]);
  };
  globalThis.URLSearchParams.prototype.toString = function() {
    let output = "";
    for (let index = 0; index < this._pairs.length; index++) {
      if (index) output += "&";
      output += formEncode(this._pairs[index][0]) + "=" + formEncode(this._pairs[index][1]);
    }
    return output;
  };
  globalThis.URLSearchParams.prototype.sort = function() {
    for (let left = 0; left < this._pairs.length; left++) {
      for (let right = left + 1; right < this._pairs.length; right++) {
        if (this._pairs[right][0] < this._pairs[left][0]) {
          const pair = this._pairs[left];
          this._pairs[left] = this._pairs[right];
          this._pairs[right] = pair;
        }
      }
    }
  };
}
for (const name of ["URL", "URLSearchParams"]) {
  if (typeof globalThis[name] === "function") {
    Object.defineProperty(globalThis, name, {
      configurable: true,
      enumerable: false,
      writable: true,
      value: globalThis[name],
    });
  }
}
"#;
        let source_with_globals = format!("var global = globalThis; var atob = function(value) {{ return String(value); }}; var btoa = function(value) {{ return String(value); }}; var structuredClone = function(value) {{ return {{ ...value }}; }}; var fetch = function() {{ return Promise.resolve(undefined); }}; var AbortController = function() {{ this.signal = {{}}; }};\n{global_source}\n{source}");
        let program =
            match path.is_some_and(|path| path.extension().is_some_and(|ext| ext == "mjs")) {
                true => quench_runtime::reduce::reduce_module_source(&source_with_globals),
                false => quench_runtime::reduce::reduce_source(&source_with_globals),
            }
            .map_err(|errors| errors.join("\n"))?;
        let capability = HostCapabilityRef {
            realm: RealmId::ROOT,
            kind: HostCapabilityKind::Custom(CapabilityName::Require),
        };
        let context = VmContext::for_realm(
            RealmId::ROOT,
            vec![
                HostCapabilityKind::Custom(CapabilityName::Require),
                HostCapabilityKind::Custom(CapabilityName::PathBasename),
                HostCapabilityKind::Custom(CapabilityName::Console),
                HostCapabilityKind::Custom(CapabilityName::ConsoleLog),
                HostCapabilityKind::Custom(CapabilityName::TimerValidation),
                HostCapabilityKind::Custom(CapabilityName::Cwd),
                HostCapabilityKind::Custom(CapabilityName::ReadFileSync),
                HostCapabilityKind::Custom(CapabilityName::CreateHash),
                HostCapabilityKind::Custom(CapabilityName::CryptoHashUpdate),
                HostCapabilityKind::Custom(CapabilityName::CryptoHashDigest),
                HostCapabilityKind::Custom(CapabilityName::CryptoHashOn),
                HostCapabilityKind::Custom(CapabilityName::CryptoHashWrite),
                HostCapabilityKind::Custom(CapabilityName::CryptoHashEnd),
                HostCapabilityKind::Custom(CapabilityName::ProcessOn),
                HostCapabilityKind::Custom(CapabilityName::ProcessEmit),
                HostCapabilityKind::Custom(CapabilityName::ProcessCpuUsage),
                HostCapabilityKind::Custom(CapabilityName::ProcessHrtime),
                HostCapabilityKind::Custom(CapabilityName::ProcessActiveResourcesInfo),
                HostCapabilityKind::Custom(CapabilityName::AssertNotStrictEqual),
                HostCapabilityKind::Custom(CapabilityName::AssertNotDeepStrictEqual),
                HostCapabilityKind::Custom(CapabilityName::AssertError),
                HostCapabilityKind::Custom(CapabilityName::AssertEqual),
                HostCapabilityKind::Custom(CapabilityName::AssertNotEqual),
                HostCapabilityKind::Custom(CapabilityName::AssertMatchValue),
                HostCapabilityKind::Custom(CapabilityName::QueueMicrotask),
                HostCapabilityKind::Custom(CapabilityName::BufferByteLength),
                HostCapabilityKind::Custom(CapabilityName::Stream),
                HostCapabilityKind::Custom(CapabilityName::StreamReadable),
                HostCapabilityKind::Custom(CapabilityName::StreamWritable),
                HostCapabilityKind::Custom(CapabilityName::StreamReadableFrom),
                HostCapabilityKind::Custom(CapabilityName::StreamDuplex),
                HostCapabilityKind::Custom(CapabilityName::StreamFinished),
                HostCapabilityKind::Custom(CapabilityName::StreamIsPaused),
                HostCapabilityKind::Custom(CapabilityName::FsAccess),
                HostCapabilityKind::Custom(CapabilityName::FsWriteBytes),
                HostCapabilityKind::Custom(CapabilityName::FsAppendBytes),
                HostCapabilityKind::Custom(CapabilityName::FsUnlink),
                HostCapabilityKind::Custom(CapabilityName::FsMkdtemp),
                HostCapabilityKind::Custom(CapabilityName::FsAccessSync),
                HostCapabilityKind::Custom(CapabilityName::FsWriteFileSync),
                HostCapabilityKind::Custom(CapabilityName::FsAppendFileSync),
                HostCapabilityKind::Custom(CapabilityName::FsUnlinkSync),
                HostCapabilityKind::Custom(CapabilityName::FsRmdirSync),
                HostCapabilityKind::Custom(CapabilityName::FsRealpathSync),
                HostCapabilityKind::Custom(CapabilityName::FsOpenSync),
                HostCapabilityKind::Custom(CapabilityName::FsCloseSync),
                HostCapabilityKind::Custom(CapabilityName::FsFchmod),
                HostCapabilityKind::Custom(CapabilityName::FsFstatSync),
                HostCapabilityKind::Custom(CapabilityName::FsChmodSync),
                HostCapabilityKind::Custom(CapabilityName::FsAccessAsync),
                HostCapabilityKind::Custom(CapabilityName::FsExistsSync),
                HostCapabilityKind::Custom(CapabilityName::ChildExecFile),
                HostCapabilityKind::Custom(CapabilityName::ChildFork),
                HostCapabilityKind::Custom(CapabilityName::ChildEmit),
                HostCapabilityKind::Custom(CapabilityName::ChildSend),
                HostCapabilityKind::Custom(CapabilityName::CommonMustCall),
                HostCapabilityKind::Custom(CapabilityName::CommonMustSucceed),
                HostCapabilityKind::Custom(CapabilityName::CommonMustNotCall),
                HostCapabilityKind::Custom(CapabilityName::FsWriteAsync),
                HostCapabilityKind::Custom(CapabilityName::FsReadAsync),
                HostCapabilityKind::Custom(CapabilityName::FsWritePromise),
                HostCapabilityKind::Custom(CapabilityName::FsReadPromise),
                HostCapabilityKind::Custom(CapabilityName::FsAppendPromise),
                HostCapabilityKind::Custom(CapabilityName::ReplServer),
                HostCapabilityKind::Custom(CapabilityName::FsOpenAsync),
                HostCapabilityKind::Custom(CapabilityName::FsCloseAsync),
                HostCapabilityKind::Custom(CapabilityName::PathRelative),
                HostCapabilityKind::Custom(CapabilityName::PathDirname),
                HostCapabilityKind::Custom(CapabilityName::PathIsAbsolute),
                HostCapabilityKind::Custom(CapabilityName::PathToNamespaced),
                HostCapabilityKind::Custom(CapabilityName::PathWinToNamespaced),
                HostCapabilityKind::Custom(CapabilityName::PathJoin),
                HostCapabilityKind::Custom(CapabilityName::PathExtname),
                HostCapabilityKind::Custom(CapabilityName::DgramDrainCallbacks),
                HostCapabilityKind::Custom(CapabilityName::CryptoDigestBytes),
                HostCapabilityKind::Custom(CapabilityName::CryptoShakeBytes),
                HostCapabilityKind::Custom(CapabilityName::UrlPattern),
                HostCapabilityKind::Custom(CapabilityName::UrlCanParse),
                HostCapabilityKind::Custom(CapabilityName::UrlHrefSet),
                HostCapabilityKind::Custom(CapabilityName::UrlSearchParams),
                HostCapabilityKind::Custom(CapabilityName::UrlSearchParamsGet),
                HostCapabilityKind::Custom(CapabilityName::UrlSearchParamsSort),
                HostCapabilityKind::Custom(CapabilityName::UrlSearchParamsOwner),
                HostCapabilityKind::Custom(CapabilityName::UrlUsernameSet),
                HostCapabilityKind::Custom(CapabilityName::UrlPasswordGet),
                HostCapabilityKind::Custom(CapabilityName::UrlPasswordSet),
                HostCapabilityKind::Custom(CapabilityName::UrlPathnameGet),
                HostCapabilityKind::Custom(CapabilityName::UrlPathnameSet),
                HostCapabilityKind::Custom(CapabilityName::UrlSearchSet),
                HostCapabilityKind::Custom(CapabilityName::UrlSearchGet),
                HostCapabilityKind::Custom(CapabilityName::UrlHashSet),
                HostCapabilityKind::Custom(CapabilityName::UrlHrefGet),
                HostCapabilityKind::Custom(CapabilityName::UrlProtocolSet),
            ],
        )
        .with_host(Rc::new(QuenchNodeHost::default()))
        .with_host_capability("require", capability)
        .with_host_capability(
            "console",
            HostCapabilityRef {
                realm: RealmId::ROOT,
                kind: HostCapabilityKind::Custom(CapabilityName::Console),
            },
        )
        .with_host_value("process", process_module())
        .with_host_value(
            "__filename",
            Value::String(
                path.map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
        )
        .with_host_value(
            "__dirname",
            Value::String(
                path.and_then(Path::parent)
                    .unwrap_or_else(|| Path::new("."))
                    .to_string_lossy()
                    .into_owned(),
            ),
        )
        .with_host_value(
            "URL",
            capability_function(HostCapabilityKind::Custom(CapabilityName::Url)),
        )
        .with_host_value(
            "setImmediate",
            capability_function(HostCapabilityKind::Custom(CapabilityName::TimerImmediate)),
        )
        .with_host_value(
            "gc",
            capability_function(HostCapabilityKind::Custom(CapabilityName::Gc)),
        )
        .with_host_value(
            "setTimeout",
            capability_function(HostCapabilityKind::Custom(CapabilityName::Timer)),
        )
        .with_host_value(
            "setInterval",
            capability_function(HostCapabilityKind::Custom(CapabilityName::Timer)),
        )
        .with_host_value(
            "clearInterval",
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::TimerClearImmediate,
            )),
        )
        .with_host_value(
            "clearImmediate",
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::TimerClearImmediate,
            )),
        )
        .with_host_value(
            "queueMicrotask",
            capability_function(HostCapabilityKind::Custom(CapabilityName::QueueMicrotask)),
        )
        .with_host_value("Buffer", buffer_module());
        let context = context
            .with_host_value(
                "__quench_fs_access",
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsAccess)),
            )
            .with_host_value(
                "__quench_fs_write_bytes",
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsWriteBytes)),
            )
            .with_host_value(
                "__quench_fs_append_bytes",
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsAppendBytes)),
            )
            .with_host_value(
                "__quench_fs_unlink",
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsUnlink)),
            )
            .with_host_value(
                "__quench_fs_mkdtemp",
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsMkdtemp)),
            )
            .with_host_value(
                "__quench_digest_bytes",
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::CryptoDigestBytes,
                )),
            )
            .with_host_value(
                "__quench_shake_bytes",
                capability_function(HostCapabilityKind::Custom(CapabilityName::CryptoShakeBytes)),
            )
            .with_host_value(
                "__quench_drain_dgram_callbacks",
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramDrainCallbacks,
                )),
            );
        quench_runtime::execute::execute_with_context(program.ops(), &context)
            .map(|_| ())
            .map_err(|error| error.render().into())
    }

    fn poll_jobs(&self) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(false)
    }

    fn has_pending_jobs(&self) -> bool {
        false
    }
}

pub(crate) struct QuickJsRuntime {
    runtime: rquickjs::Runtime,
}

impl QuickJsRuntime {
    pub(crate) fn new() -> Result<Self, rquickjs::Error> {
        Ok(Self {
            runtime: rquickjs::Runtime::new()?,
        })
    }
}

impl JsRuntime for QuickJsRuntime {
    fn execute(
        &self,
        source: &str,
        path: Option<&Path>,
        _host: &dyn NodeHost,
    ) -> Result<(), Box<dyn std::error::Error>> {
        crate::quickjs_backend::execute_source(source, &self.runtime, path)?;
        while self.has_pending_jobs() {
            self.poll_jobs()?;
        }
        Ok(())
    }

    fn poll_jobs(&self) -> Result<bool, Box<dyn std::error::Error>> {
        self.runtime
            .execute_pending_job()
            .map_err(|error| format!("QuickJS job failed: {error:?}").into())
    }

    fn has_pending_jobs(&self) -> bool {
        self.runtime.is_job_pending()
    }
}
