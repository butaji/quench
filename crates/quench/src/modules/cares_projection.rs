//! One Node record schema over the authoritative typed native RR data.
use super::channel::{QueryType, RData, Record};
#[derive(Debug)]
pub enum Js {
    Undefined,
    Number(f64),
    Text(Vec<u8>),
    Array(Vec<Js>),
    Object(Vec<(String, Js)>),
    Binary(Vec<u8>),
}
fn text(value: &str) -> Js {
    Js::Text(value.as_bytes().to_vec())
}
fn object(fields: Vec<(&str, Js)>) -> Js {
    Js::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}
fn number(value: impl Into<f64>) -> Js {
    Js::Number(value.into())
}
fn kind(data: &RData) -> QueryType {
    match data {
        RData::A(_) => QueryType::A,
        RData::Aaaa(_) => QueryType::Aaaa,
        RData::Cname(_) => QueryType::Cname,
        RData::Ns(_) => QueryType::Ns,
        RData::Ptr(_) => QueryType::Ptr,
        RData::Mx { .. } => QueryType::Mx,
        RData::Txt(_) => QueryType::Txt,
        RData::Srv { .. } => QueryType::Srv,
        RData::Soa { .. } => QueryType::Soa,
        RData::Naptr { .. } => QueryType::Naptr,
        RData::Caa { .. } => QueryType::Caa,
        RData::Tlsa { .. } => QueryType::Tlsa,
    }
}
fn any_rank(data: &RData) -> usize {
    match kind(data) {
        QueryType::A | QueryType::Cname => 0,
        QueryType::Aaaa => 1,
        QueryType::Mx => 2,
        QueryType::Ns => 3,
        QueryType::Txt => 4,
        QueryType::Srv => 5,
        QueryType::Ptr => 6,
        QueryType::Naptr => 7,
        QueryType::Soa => 8,
        QueryType::Tlsa => 9,
        QueryType::Caa => 10,
        QueryType::Any => unreachable!(),
    }
}
pub fn arguments(query: QueryType, mut records: Vec<Record>) -> Vec<Js> {
    let any = query == QueryType::Any;
    if any {
        records.sort_by_key(|record| any_rank(&record.data));
        let has_a = records
            .iter()
            .any(|record| matches!(record.data, RData::A(_)));
        if has_a {
            records.retain(|record| !matches!(record.data, RData::Cname(_)));
        }
    } else {
        records.retain(|record| kind(&record.data) == query);
    }
    let ttls = records.iter().map(|record| number(record.ttl)).collect();
    let mut values: Vec<Js> = records
        .into_iter()
        .map(|record| {
            let ttl = record.ttl;
            match record.data {
                RData::A(ip) => {
                    if any {
                        object(vec![
                            ("address", text(&ip.to_string())),
                            ("ttl", number(ttl)),
                            ("type", text("A")),
                        ])
                    } else {
                        text(&ip.to_string())
                    }
                }
                RData::Aaaa(ip) => {
                    if any {
                        object(vec![
                            ("address", text(&ip.to_string())),
                            ("ttl", number(ttl)),
                            ("type", text("AAAA")),
                        ])
                    } else {
                        text(&ip.to_string())
                    }
                }
                RData::Cname(name) => {
                    if any {
                        object(vec![("value", Js::Text(name)), ("type", text("CNAME"))])
                    } else {
                        Js::Text(name)
                    }
                }
                RData::Ns(name) => {
                    if any {
                        object(vec![("value", Js::Text(name)), ("type", text("NS"))])
                    } else {
                        Js::Text(name)
                    }
                }
                RData::Ptr(name) => {
                    if any {
                        object(vec![("value", Js::Text(name)), ("type", text("PTR"))])
                    } else {
                        Js::Text(name)
                    }
                }
                RData::Mx { priority, exchange } => object(vec![
                    ("exchange", Js::Text(exchange)),
                    ("priority", number(priority)),
                    ("type", text("MX")),
                ]),
                RData::Txt(parts) => {
                    let entries = Js::Array(parts.into_iter().map(Js::Text).collect());
                    if any {
                        object(vec![("entries", entries), ("type", text("TXT"))])
                    } else {
                        entries
                    }
                }
                RData::Srv {
                    priority,
                    weight,
                    port,
                    name,
                } => object(vec![
                    ("name", Js::Text(name)),
                    ("port", number(port)),
                    ("priority", number(priority)),
                    ("weight", number(weight)),
                    ("type", text("SRV")),
                ]),
                RData::Soa {
                    nsname,
                    hostmaster,
                    serial,
                    refresh,
                    retry,
                    expire,
                    minttl,
                } => object(vec![
                    ("nsname", Js::Text(nsname)),
                    ("hostmaster", Js::Text(hostmaster)),
                    ("serial", number(serial)),
                    ("refresh", number(refresh as i32)),
                    ("retry", number(retry as i32)),
                    ("expire", number(expire as i32)),
                    ("minttl", number(minttl)),
                    ("type", if any { text("SOA") } else { Js::Undefined }),
                ]),
                RData::Naptr {
                    order,
                    preference,
                    flags,
                    service,
                    regexp,
                    replacement,
                } => object(vec![
                    ("flags", Js::Text(flags)),
                    ("service", Js::Text(service)),
                    ("regexp", Js::Text(regexp)),
                    ("replacement", Js::Text(replacement)),
                    ("order", number(order)),
                    ("preference", number(preference)),
                    ("type", if any { text("NAPTR") } else { Js::Undefined }),
                ]),
                RData::Caa {
                    critical,
                    tag,
                    value,
                } => {
                    let mut fields = vec![
                        ("critical".to_owned(), number(critical)),
                        ("type".to_owned(), text("CAA")),
                    ];
                    fields.push((
                        tag.iter().map(|byte| char::from(*byte)).collect(),
                        Js::Text(value),
                    ));
                    Js::Object(fields)
                }
                RData::Tlsa {
                    usage,
                    selector,
                    matching_type,
                    data,
                } => object(vec![
                    ("certUsage", number(usage)),
                    ("selector", number(selector)),
                    ("match", number(matching_type)),
                    ("data", Js::Binary(data)),
                ]),
            }
        })
        .collect();
    let result = if query == QueryType::Soa {
        if values.is_empty() {
            Js::Undefined
        } else {
            values.remove(0)
        }
    } else {
        Js::Array(values)
    };
    let mut args = vec![number(0), result];
    if matches!(query, QueryType::A | QueryType::Aaaa) {
        args.push(Js::Array(ttls));
    }
    args
}
