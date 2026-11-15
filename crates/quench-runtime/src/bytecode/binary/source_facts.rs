//! A program source's encoded facts: its origin, its call sites and its magic comments.
use super::{BinaryReader, BinaryWriter, read_optional_string, write_optional_string};

pub(super) fn write_source_facts(out: &mut BinaryWriter, source: &crate::bytecode::ProgramSource) {
    out.bool(source.typescript);
    out.u32(source.base);
    out.u32(source.line_offset);
    out.u32(source.column_offset);
    out.bool(source.call_sites.get().is_some());
    if let Some(sites) = source.call_sites.get() {
        out.u32(sites.len() as u32);
        for site in sites.iter() {
            out.u32(site.offset);
            out.bool(site.construct);
            out.string(&site.callee);
            out.bool(site.expression.is_some());
            if let Some(span) = site.expression {
                out.u32(span.start);
                out.u32(span.end);
            }
        }
    }
    write_optional_string(out, source.directives.source_url.as_deref());
    write_optional_string(out, source.directives.source_map_url.as_deref());
}

pub(super) fn read_source_facts(input: &mut BinaryReader<'_>, text: crate::bytecode::SourceText) -> Result<crate::bytecode::ProgramSource, String> {
    let typescript = input.bool()?;
    let (base, line_offset, column_offset) = (input.u32()?, input.u32()?, input.u32()?);
    let call_sites = if input.bool()? {
        std::cell::OnceCell::from(std::sync::Arc::from(input.list(|input| {
            Ok(crate::bytecode::CallSiteDescription {
                offset: input.u32()?,
                construct: input.bool()?,
                callee: input.string()?,
                expression: if input.bool()? {
                    Some(crate::bytecode::SourceSpan { start: input.u32()?, end: input.u32()? })
                } else { None },
            })
        })?))
    } else {
        Default::default()
    };
    let directives = crate::bytecode::SourceDirectives {
        source_url: read_optional_string(input)?.map(Into::into),
        source_map_url: read_optional_string(input)?.map(Into::into),
    };
    Ok(crate::bytecode::ProgramSource { text, typescript, base, line_offset, column_offset, line_starts: Default::default(), call_sites, directives })
}
