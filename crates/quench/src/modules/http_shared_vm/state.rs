//! The host side of `http`: the message parsers its connections read with, by id, and the
//! module's intrinsics.

use super::parser::Parser;
use super::Intrinsics;
use rustc_hash::FxHashMap;

#[derive(Default)]
pub(crate) struct State {
    next_parser: u64,
    pub(crate) parsers: FxHashMap<u64, Parser>,
    pub(crate) intrinsics: Option<Intrinsics>,
}

impl State {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn add_parser(&mut self, parser: Parser) -> u64 {
        let id = self.next_parser;
        self.next_parser += 1;
        self.parsers.insert(id, parser);
        id
    }

    pub(crate) fn write_snapshot(&self, writer: &mut quench_runtime::SnapshotWriter<'_>) -> Result<(), String> {
        use crate::host::startup_snapshot::{require_quiescent, HostSnap};
        let Self { next_parser, parsers, intrinsics } = self;
        require_quiescent(parsers.is_empty(), "an HTTP connection")?;
        next_parser.write(writer)?;
        intrinsics.write(writer)
    }

    pub(crate) fn read_snapshot(&mut self, reader: &mut quench_runtime::SnapshotReader<'_, '_>) -> Result<(), String> {
        use crate::host::startup_snapshot::HostSnap;
        self.next_parser = HostSnap::read(reader)?;
        self.parsers.clear();
        self.intrinsics = HostSnap::read(reader)?;
        Ok(())
    }
}
