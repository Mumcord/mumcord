//! DNS SRV lookup for Mumble servers (issue #859).
//!
//! A server may publish its real host and port as `_mumble._tcp.<domain>`, so users can join it
//! by entering just the domain. The official Mumble client resolves these records; Mumcord does
//! the same for addresses that use the default port. An explicit, non-default port always wins.
//!
//! Only where the client opens its sockets changes: the saved server, its certificate trust and
//! its identity stay tied to the address the user entered.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use hickory_resolver::TokioResolver;
use hickory_resolver::config::{ConnectionConfig, NameServerConfig, ResolverConfig};
use hickory_resolver::net::NetError;
use hickory_resolver::net::runtime::TokioRuntimeProvider;
use hickory_resolver::proto::rr::RData;
use hickory_resolver::proto::rr::rdata::SRV;
use mumble_client_protocol::ConnectError;
use mumcord_store::DEFAULT_PORT;
use ring::rand::{SecureRandom, SystemRandom};

/// Upper bound for the SRV lookup; a slow or broken DNS server must not hold up the connect
/// for long, the plain address is tried afterwards.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(3);

/// Where a connection is actually opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub host: String,
    pub port: u16,
}

/// The hosts to try for `host:port`, best first: the targets of its `_mumble._tcp` SRV records
/// when the port is the default one and records exist, otherwise just `host:port`. Never empty.
pub async fn targets(host: &str, port: u16) -> Vec<Target> {
    if !wants_lookup(host, port) {
        return direct(host, port);
    }
    targets_via(resolver().as_ref(), host, port).await
}

/// [`targets`] with a given resolver; `None` when there is no DNS configuration to use.
async fn targets_via(resolver: Option<&TokioResolver>, host: &str, port: u16) -> Vec<Target> {
    let found = match resolver {
        Some(resolver) if wants_lookup(host, port) => lookup(resolver, host).await,
        _ => Vec::new(),
    };
    if found.is_empty() {
        direct(host, port)
    } else {
        found
    }
}

fn direct(host: &str, port: u16) -> Vec<Target> {
    vec![Target {
        host: host.to_owned(),
        port,
    }]
}

/// The system resolver, built per lookup so a changed network (other DNS servers) is picked up
/// on reconnect. Debug builds can point it at a local DNS fixture for the live tests.
fn resolver() -> Option<TokioResolver> {
    let built = match crate::test_hooks::dns_server() {
        Some(server) => resolver_at(server),
        None => TokioResolver::builder_tokio().and_then(|builder| builder.build()),
    };
    match built {
        Ok(resolver) => Some(resolver),
        Err(e) => {
            tracing::debug!(error = %e, "no DNS configuration; skipping the SRV lookup");
            None
        }
    }
}

/// A resolver that asks only the DNS server at `server`.
fn resolver_at(server: SocketAddr) -> Result<TokioResolver, NetError> {
    let mut connection = ConnectionConfig::udp();
    connection.port = server.port();
    let name_server = NameServerConfig::new(server.ip(), true, vec![connection]);
    TokioResolver::builder_with_config(
        ResolverConfig::from_name_servers(vec![name_server]),
        TokioRuntimeProvider::default(),
    )
    .build()
}

/// Whether a connect failed before it reached a Mumble server at the target, so the next SRV
/// target is worth a try. Any other failure (certificate, password, refusal) is final.
pub fn unreachable(err: &ConnectError) -> bool {
    matches!(
        err,
        ConnectError::Resolve(_) | ConnectError::Io(_) | ConnectError::Timeout
    )
}

/// Only a host name entered without a port (stored as the default port) gets a lookup. IP
/// addresses and `localhost` never have SRV records worth asking DNS for.
fn wants_lookup(host: &str, port: u16) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    port == DEFAULT_PORT
        && !host.is_empty()
        && bare.parse::<IpAddr>().is_err()
        && !host.eq_ignore_ascii_case("localhost")
}

/// The ordered targets of `_mumble._tcp.<host>`; empty when there are none or the lookup failed.
async fn lookup(resolver: &TokioResolver, host: &str) -> Vec<Target> {
    let name = format!("_mumble._tcp.{}.", host.trim_end_matches('.'));
    let records = match tokio::time::timeout(LOOKUP_TIMEOUT, resolver.srv_lookup(name)).await {
        Ok(Ok(lookup)) => lookup
            .answers()
            .iter()
            .filter_map(|record| match &record.data {
                RData::SRV(srv) => Some(srv.clone()),
                _ => None,
            })
            .collect(),
        Ok(Err(e)) => {
            if !e.is_no_records_found() {
                tracing::debug!(error = %e, "the SRV lookup failed; using the address as entered");
            }
            return Vec::new();
        }
        Err(_) => {
            tracing::debug!("the SRV lookup timed out; using the address as entered");
            return Vec::new();
        }
    };
    order(records, &mut random_below)
}

/// RFC 2782 order: lowest priority first; within one priority a weighted random order, where a
/// record of weight 0 only comes after the weighted ones. `random(n)` returns a value below `n`.
fn order(mut records: Vec<SRV>, random: &mut impl FnMut(u32) -> u32) -> Vec<Target> {
    // A target of "." means the service is not offered under this name; the address as entered
    // is tried instead, which is what the user would get without the record.
    records.retain(|record| !record.target.is_root());
    records.sort_by_key(|record| record.priority);
    let mut ordered = Vec::with_capacity(records.len());
    for group in records.chunk_by(|a, b| a.priority == b.priority) {
        let mut group: Vec<&SRV> = group.iter().collect();
        while !group.is_empty() {
            let total: u32 = group.iter().map(|r| u32::from(r.weight)).sum();
            let index = if total == 0 {
                0
            } else {
                let mut pick = random(total);
                group
                    .iter()
                    .position(|r| {
                        let weight = u32::from(r.weight);
                        if pick < weight {
                            true
                        } else {
                            pick -= weight;
                            false
                        }
                    })
                    .unwrap_or(0)
            };
            let record = group.remove(index);
            ordered.push(Target {
                host: record.target.to_ascii().trim_end_matches('.').to_owned(),
                port: record.port,
            });
        }
    }
    ordered
}

/// A value below `bound` (> 0). Only spreads load between equal servers, so a failing random
/// source just picks the first one.
fn random_below(bound: u32) -> u32 {
    let mut bytes = [0u8; 4];
    if SystemRandom::new().fill(&mut bytes).is_err() {
        return 0;
    }
    u32::from_le_bytes(bytes) % bound
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use hickory_resolver::proto::op::{Message, ResponseCode};
    use hickory_resolver::proto::rr::{Name, Record};
    use hickory_resolver::proto::serialize::binary::{BinDecodable, BinEncodable};
    use tokio::net::UdpSocket;

    use super::*;

    fn srv(priority: u16, weight: u16, port: u16, target: &str) -> SRV {
        SRV::new(
            priority,
            weight,
            port,
            Name::from_ascii(target).expect("name"),
        )
    }

    fn target(host: &str, port: u16) -> Target {
        Target {
            host: host.to_owned(),
            port,
        }
    }

    /// A local DNS server that answers every SRV query for `_mumble._tcp.<zone>.` with `records`
    /// and everything else with NXDOMAIN. No public DNS is involved.
    async fn dns_fixture(zone: &'static str, records: Vec<SRV>) -> SocketAddr {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let addr = socket.local_addr().expect("addr");
        tokio::spawn(async move {
            let mut buffer = [0u8; 1500];
            loop {
                let Ok((len, from)) = socket.recv_from(&mut buffer).await else {
                    return;
                };
                let Ok(query) = Message::from_bytes(&buffer[..len]) else {
                    continue;
                };
                let mut reply = Message::response(query.metadata.id, query.metadata.op_code);
                reply.metadata.recursion_desired = query.metadata.recursion_desired;
                reply.metadata.recursion_available = true;
                reply.add_queries(query.queries.clone());
                let asked = query.queries.first().map(|q| q.name().clone());
                let wanted = Name::from_ascii(format!("_mumble._tcp.{zone}.")).expect("name");
                match asked {
                    Some(name) if name == wanted && !records.is_empty() => {
                        for record in &records {
                            reply.add_answer(Record::from_rdata(
                                name.clone(),
                                60,
                                RData::SRV(record.clone()),
                            ));
                        }
                    }
                    _ => {
                        reply.metadata.response_code = ResponseCode::NXDomain;
                    }
                }
                let bytes = reply.to_bytes().expect("encode");
                let _ = socket.send_to(&bytes, from).await;
            }
        });
        addr
    }

    fn resolver_for(dns: SocketAddr) -> TokioResolver {
        resolver_at(dns).expect("resolver")
    }

    #[test]
    fn only_host_names_on_the_default_port_are_looked_up() {
        assert!(wants_lookup("example.org", DEFAULT_PORT));
        assert!(!wants_lookup("example.org", 12345));
        assert!(!wants_lookup("192.0.2.1", DEFAULT_PORT));
        assert!(!wants_lookup("2001:db8::1", DEFAULT_PORT));
        assert!(!wants_lookup("[2001:db8::1]", DEFAULT_PORT));
        assert!(!wants_lookup("localhost", DEFAULT_PORT));
        assert!(!wants_lookup("", DEFAULT_PORT));
    }

    #[test]
    fn lower_priority_comes_first() {
        let records = vec![
            srv(20, 0, 3, "backup.example.org."),
            srv(10, 0, 1, "main.example.org."),
        ];
        assert_eq!(
            order(records, &mut |_| 0),
            vec![
                target("main.example.org", 1),
                target("backup.example.org", 3)
            ]
        );
    }

    #[test]
    fn weight_decides_within_one_priority() {
        let records = vec![
            srv(10, 1, 1, "light.example.org."),
            srv(10, 9, 2, "heavy.example.org."),
        ];
        // A pick of 0 falls into the first record's share, any pick from 1 to 9 into the second.
        assert_eq!(
            order(records.clone(), &mut |_| 0),
            vec![
                target("light.example.org", 1),
                target("heavy.example.org", 2)
            ]
        );
        assert_eq!(
            order(records, &mut |_| 5),
            vec![
                target("heavy.example.org", 2),
                target("light.example.org", 1)
            ]
        );
    }

    #[test]
    fn zero_weight_comes_after_weighted_records() {
        let records = vec![
            srv(10, 0, 1, "zero.example.org."),
            srv(10, 5, 2, "weighted.example.org."),
        ];
        assert_eq!(
            order(records, &mut |_| 0),
            vec![
                target("weighted.example.org", 2),
                target("zero.example.org", 1)
            ]
        );
    }

    #[test]
    fn a_root_target_means_no_service() {
        assert!(order(vec![srv(0, 0, 1, ".")], &mut |_| 0).is_empty());
    }

    #[tokio::test]
    async fn a_published_record_gives_its_target() {
        let dns = dns_fixture("srv.test", vec![srv(0, 0, 12345, "voice.srv.test.")]).await;
        assert_eq!(
            lookup(&resolver_for(dns), "srv.test").await,
            vec![target("voice.srv.test", 12345)]
        );
    }

    #[tokio::test]
    async fn no_record_gives_nothing() {
        let dns = dns_fixture("srv.test", Vec::new()).await;
        assert!(lookup(&resolver_for(dns), "srv.test").await.is_empty());
        assert!(lookup(&resolver_for(dns), "other.test").await.is_empty());
    }

    #[tokio::test]
    async fn without_a_record_the_entered_address_is_used() {
        let dns = dns_fixture("srv.test", Vec::new()).await;
        assert_eq!(
            targets_via(Some(&resolver_for(dns)), "srv.test", DEFAULT_PORT).await,
            vec![target("srv.test", DEFAULT_PORT)]
        );
        assert_eq!(
            targets_via(None, "srv.test", DEFAULT_PORT).await,
            vec![target("srv.test", DEFAULT_PORT)]
        );
    }

    #[tokio::test]
    async fn an_explicit_port_wins_over_a_record() {
        let dns = dns_fixture("srv.test", vec![srv(0, 0, 12345, "voice.srv.test.")]).await;
        let resolver = resolver_for(dns);
        assert_eq!(
            targets_via(Some(&resolver), "srv.test", DEFAULT_PORT).await,
            vec![target("voice.srv.test", 12345)]
        );
        assert_eq!(
            targets_via(Some(&resolver), "srv.test", 4000).await,
            vec![target("srv.test", 4000)]
        );
    }

    #[tokio::test]
    async fn an_unanswered_lookup_falls_back_to_the_entered_address() {
        // A socket that never answers: the lookup gives up and the address is used as entered.
        let silent = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let resolver = resolver_for(silent.local_addr().expect("addr"));
        assert_eq!(
            targets_via(Some(&resolver), "srv.test", DEFAULT_PORT).await,
            vec![target("srv.test", DEFAULT_PORT)]
        );
    }
}
