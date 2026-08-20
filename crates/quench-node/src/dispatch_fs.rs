//! `fs` capability dispatch table — one arm per operation.

use crate::dispatch_handlers::CallHandler;
use crate::modules::{fs_async, fs_promises, fs_stats, fs_sync};

const CAP_FS_READFILE: u16 = 0x1100;
const CAP_FS_WRITEFILE: u16 = 0x1101;
const CAP_FS_STAT: u16 = 0x1102;
const CAP_FS_READDIR: u16 = 0x1103;
const CAP_FS_EXISTS: u16 = 0x1104;
const CAP_FS_MKDIR: u16 = 0x1105;
const CAP_FS_UNLINK: u16 = 0x1106;
const CAP_FS_READFILESYNC: u16 = 0x1107;
const CAP_FS_WRITEFILESYNC: u16 = 0x1108;
const CAP_FS_STATSYNC: u16 = 0x1109;
const CAP_FS_READDIRSYNC: u16 = 0x110A;
const CAP_FS_EXISTSSYNC: u16 = 0x110B;
const CAP_FS_REALSYNC: u16 = 0x110C;
const CAP_FS_LSTAT: u16 = 0x110D;
const CAP_FS_ACCESS: u16 = 0x110E;
const CAP_FS_RMDIR: u16 = 0x110F;
const CAP_FS_RM: u16 = 0x1110;
const CAP_FS_RENAME: u16 = 0x1111;
const CAP_FS_APPENDFILE: u16 = 0x1112;
const CAP_FS_COPYFILE: u16 = 0x1113;
const CAP_FS_MKDTEMP: u16 = 0x1114;
const CAP_FS_READLINK: u16 = 0x1115;
const CAP_FS_CHMOD: u16 = 0x1116;
const CAP_FS_TRUNCATE: u16 = 0x1117;
const CAP_FS_LSTATSYNC: u16 = 0x1118;
const CAP_FS_ACCESSSYNC: u16 = 0x1119;
const CAP_FS_RMDIRSYNC: u16 = 0x111A;
const CAP_FS_RMSYNC: u16 = 0x111B;
const CAP_FS_RENAMESYNC: u16 = 0x111C;
const CAP_FS_APPENDFILESYNC: u16 = 0x111D;
const CAP_FS_COPYFILESYNC: u16 = 0x111E;
const CAP_FS_MKDTEMPSYNC: u16 = 0x111F;
const CAP_FS_READLINKSYNC: u16 = 0x1120;
const CAP_FS_CHMODSYNC: u16 = 0x1121;
const CAP_FS_TRUNCATESYNC: u16 = 0x1122;
const CAP_FS_MKDIRSYNC: u16 = 0x1123;
const CAP_FS_UNLINKSYNC: u16 = 0x1124;
const CAP_FS_OPEN: u16 = 0x1160;
const CAP_FS_FSTAT: u16 = 0x1161;
const CAP_FS_CLOSE: u16 = 0x1162;
const CAP_FS_OPENSYNC: u16 = 0x1163;
const CAP_FS_FSTATSYNC: u16 = 0x1164;
const CAP_FS_CLOSESYNC: u16 = 0x1165;
const CAP_FS_STATS: u16 = 0x1166;
const CAP_FS_STAT_ISFILE: u16 = 0x1130;
const CAP_FS_STAT_ISDIR: u16 = 0x1131;
const CAP_FS_STAT_ISSYMLINK: u16 = 0x1132;
const CAP_FS_STAT_ISBLOCK: u16 = 0x1133;
const CAP_FS_STAT_ISCHAR: u16 = 0x1134;
const CAP_FS_STAT_ISFIFO: u16 = 0x1135;
const CAP_FS_STAT_ISSOCKET: u16 = 0x1136;

const CAP_FS_REALPATH: u16 = 0x1137;
const CAP_FSP_READFILE: u16 = 0x1140;
const CAP_FSP_WRITEFILE: u16 = 0x1141;
const CAP_FSP_APPENDFILE: u16 = 0x1142;
const CAP_FSP_STAT: u16 = 0x1143;
const CAP_FSP_LSTAT: u16 = 0x1144;
const CAP_FSP_READDIR: u16 = 0x1145;
const CAP_FSP_MKDIR: u16 = 0x1146;
const CAP_FSP_UNLINK: u16 = 0x1147;
const CAP_FSP_RMDIR: u16 = 0x1148;
const CAP_FSP_RM: u16 = 0x1149;
const CAP_FSP_RENAME: u16 = 0x114A;
const CAP_FSP_COPYFILE: u16 = 0x114B;
const CAP_FSP_ACCESS: u16 = 0x114C;
const CAP_FSP_MKDTEMP: u16 = 0x114D;
const CAP_FSP_READLINK: u16 = 0x114E;
const CAP_FSP_CHMOD: u16 = 0x114F;
const CAP_FSP_TRUNCATE: u16 = 0x1150;
const CAP_FSP_REALPATH: u16 = 0x1151;
const CAP_FSP_OPEN: u16 = 0x1170;
const CAP_FSP_FILEHANDLE_STAT: u16 = 0x1171;
const CAP_FSP_FILEHANDLE_CLOSE: u16 = 0x1172;
const CAP_FSP_FILEHANDLE_TRUNCATE: u16 = 0x1173;
const CAP_FSP_FILEHANDLE_DATASYNC: u16 = 0x1174;
const CAP_FSP_FILEHANDLE_SYNC: u16 = 0x1175;
const CAP_FSP_FILEHANDLE_WRITE: u16 = 0x1176;
const CAP_FSP_FILEHANDLE_READ: u16 = 0x1177;
const CAP_FSP_FILEHANDLE_CHMOD: u16 = 0x1178;
const CAP_FSP_FILEHANDLE_CHOWN: u16 = 0x1179;
const CAP_FSP_FILEHANDLE_UTIMES: u16 = 0x117A;

pub fn fs_dispatch(cap: u16) -> Option<CallHandler> {
    Some(match cap {
        CAP_FS_READFILE => fs_async::read_file,
        CAP_FS_WRITEFILE => fs_async::write_file,
        CAP_FS_STAT => fs_async::stat,
        CAP_FS_READDIR => fs_async::readdir,
        CAP_FS_EXISTS => fs_async::exists,
        CAP_FS_MKDIR => fs_async::mkdir,
        CAP_FS_UNLINK => fs_async::unlink,
        CAP_FS_READFILESYNC => fs_sync::read_file_sync,
        CAP_FS_WRITEFILESYNC => fs_sync::write_file_sync,
        CAP_FS_STATSYNC => fs_sync::stat_sync,
        CAP_FS_READDIRSYNC => fs_sync::readdir_sync,
        CAP_FS_EXISTSSYNC => fs_sync::exists_sync,
        CAP_FS_REALSYNC => fs_sync::realpath_sync,
        CAP_FS_OPENSYNC => fs_sync::open_sync,
        CAP_FS_FSTATSYNC => fs_sync::fstat_sync,
        CAP_FS_CLOSESYNC => fs_sync::close_sync,
        CAP_FS_STATS => fs_sync::stats_constructor,
        _ => return fs_dispatch_more(cap),
    })
}

fn fs_dispatch_more(cap: u16) -> Option<CallHandler> {
    Some(match cap {
        CAP_FS_LSTAT => fs_async::lstat,
        CAP_FS_ACCESS => fs_async::access,
        CAP_FS_RMDIR => fs_async::rmdir,
        CAP_FS_RM => fs_async::rm,
        CAP_FS_RENAME => fs_async::rename,
        CAP_FS_APPENDFILE => fs_async::append_file,
        CAP_FS_COPYFILE => fs_async::copy_file,
        CAP_FS_MKDTEMP => fs_async::mkdtemp,
        CAP_FS_READLINK => fs_async::readlink,
        CAP_FS_CHMOD => fs_async::chmod,
        CAP_FS_TRUNCATE => fs_async::truncate,
        CAP_FS_LSTATSYNC => fs_sync::lstat_sync,
        CAP_FS_ACCESSSYNC => fs_sync::access_sync,
        CAP_FS_RMDIRSYNC => fs_sync::rmdir_sync,
        CAP_FS_RMSYNC => fs_sync::rm_sync,
        CAP_FS_RENAMESYNC => fs_sync::rename_sync,
        CAP_FS_APPENDFILESYNC => fs_sync::append_file_sync,
        CAP_FS_COPYFILESYNC => fs_sync::copy_file_sync,
        CAP_FS_MKDTEMPSYNC => fs_sync::mkdtemp_sync,
        CAP_FS_READLINKSYNC => fs_sync::readlink_sync,
        CAP_FS_CHMODSYNC => fs_sync::chmod_sync,
        CAP_FS_TRUNCATESYNC => fs_sync::truncate_sync,
        CAP_FS_MKDIRSYNC => fs_sync::mkdir_sync,
        CAP_FS_UNLINKSYNC => fs_sync::unlink_sync,
        CAP_FS_STAT_ISFILE => fs_stats::is_file,
        CAP_FS_STAT_ISDIR => fs_stats::is_dir,
        CAP_FS_STAT_ISSYMLINK => fs_stats::is_symlink,
        CAP_FS_STAT_ISBLOCK => fs_stats::is_block,
        CAP_FS_STAT_ISCHAR => fs_stats::is_char,
        CAP_FS_STAT_ISFIFO => fs_stats::is_fifo,
        CAP_FS_OPEN => fs_async::open,
        CAP_FS_FSTAT => fs_async::fstat,
        CAP_FS_CLOSE => fs_async::close,
        CAP_FS_STAT_ISSOCKET => fs_stats::is_socket,
        _ => return fs_dispatch_promises(cap),
    })
}

fn fs_dispatch_promises(cap: u16) -> Option<CallHandler> {
    Some(match cap {
        CAP_FS_REALPATH => fs_async::realpath,
        CAP_FSP_READFILE => fs_promises::read_file,
        CAP_FSP_WRITEFILE => fs_promises::write_file,
        CAP_FSP_APPENDFILE => fs_promises::append_file,
        CAP_FSP_STAT => fs_promises::stat,
        CAP_FSP_LSTAT => fs_promises::lstat,
        CAP_FSP_READDIR => fs_promises::readdir,
        CAP_FSP_MKDIR => fs_promises::mkdir,
        CAP_FSP_UNLINK => fs_promises::unlink,
        CAP_FSP_RMDIR => fs_promises::rmdir,
        CAP_FSP_RM => fs_promises::rm,
        CAP_FSP_RENAME => fs_promises::rename,
        CAP_FSP_COPYFILE => fs_promises::copy_file,
        CAP_FSP_ACCESS => fs_promises::access,
        CAP_FSP_MKDTEMP => fs_promises::mkdtemp,
        CAP_FSP_READLINK => fs_promises::readlink,
        CAP_FSP_CHMOD => fs_promises::chmod,
        CAP_FSP_TRUNCATE => fs_promises::truncate,
        CAP_FSP_REALPATH => fs_promises::realpath,
        CAP_FSP_OPEN => fs_promises::open,
        CAP_FSP_FILEHANDLE_STAT => fs_promises::filehandle_stat,
        CAP_FSP_FILEHANDLE_CLOSE => fs_promises::filehandle_close,
        CAP_FSP_FILEHANDLE_TRUNCATE => fs_promises::filehandle_truncate,
        CAP_FSP_FILEHANDLE_DATASYNC => fs_promises::filehandle_datasync,
        CAP_FSP_FILEHANDLE_SYNC => fs_promises::filehandle_sync,
        CAP_FSP_FILEHANDLE_WRITE => fs_promises::filehandle_write,
        CAP_FSP_FILEHANDLE_READ => fs_promises::filehandle_read,
        CAP_FSP_FILEHANDLE_CHMOD => fs_promises::filehandle_chmod,
        CAP_FSP_FILEHANDLE_CHOWN => fs_promises::filehandle_chown,
        CAP_FSP_FILEHANDLE_UTIMES => fs_promises::filehandle_utimes,
        _ => return crate::dispatch::assert_dispatch(cap),
    })
}
