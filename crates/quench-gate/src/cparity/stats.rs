//! What a run reports on stderr under `QUENCH_TRACE=js-jit,jit-dump=<dir>`: the loop regions it
//! compiled and dumped, and per function the back-edges each on-stack-replacement site took.

use std::str::FromStr;

const DUMP_PREFIX: &str = "QUENCH_JS_JIT_DUMP ";
const HEADER_FIELD: &str = "header=";
const STATS_PREFIX: &str = "QUENCH_JS_JIT_STATS ";
const SITE_HEADER_KEY: &str = "\"header_pc\":";
const SITE_COUNT_KEY: &str = "\"lane_backedge_count\":";
const SITE_STATUS_KEY: &str = "\"status\":\"";
const ADMITTED: &str = "admitted";

/// A compiled loop region written as `<name>.bin` in the dump directory.
#[derive(Debug, PartialEq, Eq)]
pub struct Dump {
    pub name: String,
    /// The bytecode position of the loop the region was compiled for.
    pub header: u32,
}

/// The leading digits of `text`, read as a number.
fn number<T: FromStr>(text: &str) -> Option<T> {
    text.chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok()
}

fn dump(line: &str) -> Option<Dump> {
    let mut fields = line.strip_prefix(DUMP_PREFIX)?.split_whitespace();
    let name = fields.next()?.to_owned();
    let header = fields.find_map(|field| number(field.strip_prefix(HEADER_FIELD)?))?;
    Some(Dump { name, header })
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

/// The header of the compiled loop that took the most back-edges: the hot loop of the program.
pub fn hot_header(stderr: &str) -> Option<u32> {
    stderr
        .lines()
        .filter_map(|line| line.strip_prefix(STATS_PREFIX))
        .flat_map(sites)
        .max_by_key(|&(_, count)| count)
        .map(|(header, _)| header)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STDERR: &str = concat!(
        "QUENCH_JS_JIT_DUMP loop-000 header=7 address=0x7fb23c000000 bytes=1280\n",
        "QUENCH_JS_JIT_DUMP loop-001 header=4 unpublished bytes=422\n",
        "QUENCH_JS_JIT_STATS {\"function_name\":\"sum\",\"osr\":{\"sites\":[{\"header_pc\":2,\"backedge_pc\":10,\"status\":\"admitted\",\"lane_backedge_count\":600},",
        "{\"header_pc\":4,\"backedge_pc\":8,\"status\":\"admitted\",\"lane_backedge_count\":22600},",
        "{\"header_pc\":9,\"backedge_pc\":12,\"status\":\"rejected\",\"lane_backedge_count\":99999}]},\"status\":\"admitted\"}\n",
    );

    #[test]
    fn dumps_are_read_in_order_with_their_headers() {
        let found = dumps(STDERR);
        assert_eq!(found, [Dump { name: "loop-000".into(), header: 7 }, Dump { name: "loop-001".into(), header: 4 }]);
    }

    #[test]
    fn the_hot_header_is_the_admitted_site_with_the_most_back_edges() {
        assert_eq!(hot_header(STDERR), Some(4));
        assert_eq!(hot_header("nothing\n"), None);
    }
}
