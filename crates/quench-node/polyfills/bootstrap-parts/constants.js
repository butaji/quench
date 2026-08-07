const __quenchOriginalRequireWithConstants = globalThis.require;
const __quenchConstants = Object.freeze({
  O_RDONLY: 0,
  O_WRONLY: 1,
  O_RDWR: 2,
  O_CREAT: 64,
  O_EXCL: 128,
  O_TRUNC: 512,
  O_APPEND: 1024,
  S_IFREG: 0o100000,
  S_IFDIR: 0o040000,
  S_IRWXU: 0o700,
  S_IRUSR: 0o400,
  S_IWUSR: 0o200,
  S_IXUSR: 0o100,
  SIGINT: 2,
  SIGTERM: 15,
  SIGKILL: 9,
  SIGPIPE: 13,
  SIGUSR1: 10,
  SIGUSR2: 12,
  UV_FS_O_FILEMAP: 0,
  COPYFILE_EXCL: 1,
  COPYFILE_FICLONE: 2,
  COPYFILE_FICLONE_FORCE: 4,
});
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "constants") {
    return __quenchConstants;
  }
  return __quenchOriginalRequireWithConstants(specifier);
};
