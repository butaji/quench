//! What a run reports on stderr under `QUENCH_TRACE=js-jit,jit-dump=<dir>`: the loop regions and
//! function bodies it compiled and dumped, and per function the back-edges each
//! on-stack-replacement site took.

use std::str::FromStr;

const DUMP_PREFIX: &str = "QUENCH_JS_JIT_DUMP ";
const LOOP_KIND: &str = "loop";
const BODY_KIND: &str = "body";
const HEADER_FIELD: &str = "header=";
const FUNCTION_FIELD: &str = "function=";
const SCALAR_FIELD: &str = "scalar=";
const STATS_FUNCTION_KEY: &str = "\"function\":";
const STATS_PREFIX: &str = "QUENCH_JS_JIT_STATS ";
const SITE_HEADER_KEY: &str = "\"header_pc\":";
const SITE_COUNT_KEY: &str = "\"lane_backedge_count\":";
const SITE_STATUS_KEY: &str = "\"status\":\"";
const ADMITTED: &str = "admitted";

/// What dumped code was compiled for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Site {
    /// A loop region, by the bytecode position of its loop's header.
    Loop { header: u32 },
    /// A function's body, and the byte offset of its scalar entry where it has one.
    Body { function: u32, scalar: Option<u64> },
}

/// Compiled code written as `<name>.bin` in the dump directory.
#[derive(Debug, PartialEq, Eq)]
pub struct Dump {
    pub name: String,
    pub site: Site,
}

/// The program's hot loop: the function that holds it and its header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hot {
    pub function: u32,
    pub header: u32,
}

/// The leading digits of `text`, read as a number.
fn number<T: FromStr>(text: &str) -> Option<T> {
    text.chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok()
}

/// The value of the field `prefix` among `fields`.
fn field<T: FromStr>(fields: &[&str], prefix: &str) -> Option<T> {
    fields.iter().find_map(|field| number(field.strip_prefix(prefix)?))
}

fn dump(line: &str) -> Option<Dump> {
    let fields: Vec<&str> = line.strip_prefix(DUMP_PREFIX)?.split_whitespace().collect();
    let (&name, &kind) = (fields.first()?, fields.get(1)?);
    let site = match kind {
        LOOP_KIND => Site::Loop { header: field(&fields, HEADER_FIELD)? },
        BODY_KIND => Site::Body { function: field(&fields, FUNCTION_FIELD)?, scalar: field(&fields, SCALAR_FIELD) },
        _ => return None,
    };
    Some(Dump { name: name.to_owned(), site })
}

/// The regions a run dumped, in compilation order.
pub fn dumps(stderr: &str) -> Vec<Dump> {
    stderr.lines().filter_map(dump).collect()
}

/// The admitted sites of one function's statistics, each as its header and back-edge count.
fn sites(stats: &str) -> impl Iterator<Item = (u32, u64)> {
    stats
        .split(SITE_HEADER_KEY)
        .skip(1)
        .filter(|site| site.split_once(SITE_STATUS_KEY).is_some_and(|(_, status)| status.starts_with(ADMITTED)))
        .filter_map(|site| Some((number(site)?, number(site.split_once(SITE_COUNT_KEY)?.1)?)))
}

/// One function's statistics: its id and its admitted sites.
fn function_sites(stats: &str) -> impl Iterator<Item = (Hot, u64)> + '_ {
    let function = stats.split_once(STATS_FUNCTION_KEY).and_then(|(_, rest)| number(rest)).unwrap_or_default();
    sites(stats).map(move |(header, count)| (Hot { function, header }, count))
}

/// The admitted loop that took the most back-edges: the hot loop of the program.
pub fn hot_loop(stderr: &str) -> Option<Hot> {
    stderr
        .lines()
        .filter_map(|line| line.strip_prefix(STATS_PREFIX))
        .flat_map(function_sites)
        .max_by_key(|&(_, count)| count)
        .map(|(hot, _)| hot)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STDERR: &str = concat!(
        "QUENCH_JS_JIT_DUMP code-000 loop header=7 address=0x7fb23c000000 bytes=1280\n",
        "QUENCH_JS_JIT_DUMP code-001 loop header=4 unpublished bytes=422\n",
        "QUENCH_JS_JIT_DUMP code-002 body function=3 scalar=64 address=0x7fb23c001000 bytes=900\n",
        "QUENCH_JS_JIT_STATS {\"program\":0,\"function\":3,\"function_name\":\"sum\",\"osr\":{\"sites\":[{\"header_pc\":2,\"backedge_pc\":10,\"status\":\"admitted\",\"lane_backedge_count\":600},",
        "{\"header_pc\":4,\"backedge_pc\":8,\"status\":\"admitted\",\"lane_backedge_count\":22600},",
        "{\"header_pc\":9,\"backedge_pc\":12,\"status\":\"rejected\",\"lane_backedge_count\":99999}]},\"status\":\"admitted\"}\n",
    );

    #[test]
    fn dumps_are_read_in_order_with_their_sites() {
        let found = dumps(STDERR);
        assert_eq!(
            found,
            [
                Dump { name: "code-000".into(), site: Site::Loop { header: 7 } },
                Dump { name: "code-001".into(), site: Site::Loop { header: 4 } },
                Dump { name: "code-002".into(), site: Site::Body { function: 3, scalar: Some(64) } },
            ]
        );
    }

    #[test]
    fn the_hot_loop_is_the_admitted_site_with_the_most_back_edges() {
        assert_eq!(hot_loop(STDERR), Some(Hot { function: 3, header: 4 }));
        assert_eq!(hot_loop("nothing\n"), None);
    }
}
