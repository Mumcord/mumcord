//! The public server query behind the `mumble://` link card (issue #175).
//!
//! The card shows live figures: the round-trip time, the users online, the user limit and the
//! server version. Only the engine contacts the network, and only through the standard UDP ping
//! that answers these numbers without connecting. It runs when the link-preview privacy switch is
//! on (Settings → Chat & history); while that switch is off nothing is asked. A link from a
//! blocked user is never queried, once user blocking exists (issue #140).
//!
//! The answer is untrusted server data: it is read with a fixed layout, and the value the query
//! carried must come back unchanged, so a packet from another client is not mistaken for the
//! answer. Results are kept for the session, so a message that is rendered again does not ask the
//! server twice; a server that did not answer is remembered for a while and the user can retry.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mumcord_protocol::{
    MumbleVersion, QueryAnswer, decode_answer, encode_legacy_query, encode_query,
};
use tokio::net::UdpSocket;

use crate::session::lock;

/// No answer within this time counts as "server not reachable".
const TIMEOUT: Duration = Duration::from_secs(3);
/// A successful answer is reused for this long.
const SUCCESS_TTL: Duration = Duration::from_secs(60);
/// A server that did not answer is retried after this long.
const FAILURE_TTL: Duration = Duration::from_secs(10 * 60);
/// Largest number of servers remembered for the session.
const CACHE_LIMIT: usize = 200;
/// Largest reply accepted; the legacy answer is 24 bytes, the protobuf one is a small message.
const REPLY_LIMIT: usize = 64;

/// The figures a server answered with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerQuery {
    pub ping_ms: u32,
    pub users: u32,
    pub max_users: u32,
    pub version: MumbleVersion,
}

/// What asking one server for its figures produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryOutcome {
    /// The link-preview privacy switch is off, so nothing was asked.
    NotAllowed,
    Answered(ServerQuery),
    /// No route to the server, no answer in time, or an answer that does not fit the query.
    Unreachable,
}

#[derive(Debug, Clone)]
struct Entry {
    at: Instant,
    outcome: QueryOutcome,
}

/// Answers of the servers queried during this run.
#[derive(Debug, Default)]
pub struct ServerQueries {
    memory: Mutex<HashMap<String, Entry>>,
}

impl ServerQueries {
    /// Answer for `address:port`; `refresh` asks the server again even when it answered before.
    pub async fn query(&self, address: &str, port: u16, refresh: bool) -> QueryOutcome {
        let key = format!("{address}:{port}");
        if !refresh && let Some(hit) = self.hit(&key) {
            return hit;
        }
        let outcome = self.fetch(address, port).await;
        self.put(&key, outcome.clone());
        outcome
    }

    fn hit(&self, key: &str) -> Option<QueryOutcome> {
        let now = Instant::now();
        let entry = lock(&self.memory).get(key)?.clone();
        // Only the outcome of a real request is worth remembering; whether the privacy switch is
        // on is read from the settings on every call.
        let ttl = match &entry.outcome {
            QueryOutcome::Answered(_) => SUCCESS_TTL,
            QueryOutcome::Unreachable => FAILURE_TTL,
            QueryOutcome::NotAllowed => return None,
        };
        (now - entry.at < ttl).then_some(entry.outcome)
    }

    fn put(&self, key: &str, outcome: QueryOutcome) {
        let mut memory = lock(&self.memory);
        if memory.len() >= CACHE_LIMIT {
            let oldest = memory
                .iter()
                .filter(|(k, _)| *k != key)
                .min_by_key(|(_, entry)| entry.at)
                .map(|(k, _)| k.clone());
            if let Some(oldest) = oldest {
                memory.remove(&oldest);
            }
        }
        memory.insert(
            key.to_owned(),
            Entry {
                at: Instant::now(),
                outcome,
            },
        );
    }

    async fn fetch(&self, address: &str, port: u16) -> QueryOutcome {
        let Some(addr) = resolve(address, port).await else {
            return QueryOutcome::Unreachable;
        };
        let socket = match UdpSocket::bind(socket_for(addr)).await {
            Ok(socket) => socket,
            Err(e) => {
                tracing::debug!(error = %e, "opening the query socket failed");
                return QueryOutcome::Unreachable;
            }
        };
        // Connecting the socket means only this server can answer, and the answer cannot come
        // from another client's packet.
        if let Err(e) = socket.connect(addr).await {
            tracing::debug!(error = %e, "the query socket could not be opened");
            return QueryOutcome::Unreachable;
        }

        // A server answers in the format its query was written in. The modern one reports the
        // exact version; the legacy one is what servers before 1.5 understand.
        let echo = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let mut answered = ask(&socket, &encode_query(echo), echo).await;
        if answered.is_none() {
            answered = ask(&socket, &encode_legacy_query(echo), echo).await;
        }
        let Some((answer, ping)) = answered else {
            tracing::debug!(address, port, "the server did not answer the query");
            return QueryOutcome::Unreachable;
        };
        QueryOutcome::Answered(ServerQuery {
            ping_ms: ping.as_millis() as u32,
            users: answer.users,
            max_users: answer.max_users,
            version: answer.version,
        })
    }
}

/// Sends one query and waits for the answer; `None` when nothing fits the request.
async fn ask(socket: &UdpSocket, query: &[u8], echo: u64) -> Option<(QueryAnswer, Duration)> {
    let started = Instant::now();
    if let Err(e) = socket.send(query).await {
        tracing::debug!(error = %e, "sending the query failed");
        return None;
    }
    let mut buffer = [0u8; REPLY_LIMIT];
    let reply = match tokio::time::timeout(TIMEOUT, socket.recv(&mut buffer)).await {
        Ok(Ok(n)) => n,
        Ok(Err(e)) => {
            tracing::debug!(error = %e, "the query got no answer");
            return None;
        }
        Err(_) => {
            tracing::debug!("the server did not answer the query in time");
            return None;
        }
    };
    let answer = decode_answer(&buffer[..reply])?;
    if answer.echo != echo {
        tracing::debug!("the query answer carried a value this client did not send");
        return None;
    }
    Some((answer, started.elapsed()))
}

async fn resolve(address: &str, port: u16) -> Option<SocketAddr> {
    match tokio::net::lookup_host((address, port)).await {
        Ok(targets) => targets.into_iter().next(),
        Err(e) => {
            tracing::debug!(address, error = %e, "resolving the query address failed");
            None
        }
    }
}

/// The bind address of the query socket: the same family as the server, so a v6-only network
/// works and an IPv4 server is not reached over a socket that would never get its answer.
fn socket_for(addr: SocketAddr) -> SocketAddr {
    match addr.ip() {
        IpAddr::V4(_) => SocketAddr::from(([0, 0, 0, 0], 0)),
        IpAddr::V6(_) => SocketAddr::from(([0u8; 16], 0)),
    }
}

#[cfg(test)]
mod tests {
    use prost::Message;

    use super::*;
    use mumcord_protocol::{LEGACY_QUERY_LEN, udp_proto};

    /// A stand-in for murmur: it answers in the format the query was written in, like the server
    /// does. `legacy_only` stands for a server before 1.5, which cannot read the modern query.
    async fn fake_server(
        version: MumbleVersion,
        users: u32,
        max: u32,
        legacy_only: bool,
    ) -> SocketAddr {
        let socket = UdpSocket::bind("127.0.0.1:0").await.expect("bind");
        let addr = socket.local_addr().expect("addr");
        tokio::spawn(async move {
            let mut buffer = [0u8; 64];
            while let Ok((n, from)) = socket.recv_from(&mut buffer).await {
                let incoming = &buffer[..n];
                let echo = if incoming.len() == LEGACY_QUERY_LEN && &incoming[..4] == &[0, 0, 0, 0]
                {
                    u64::from_be_bytes(incoming[4..12].try_into().expect("echo"))
                } else if legacy_only {
                    continue;
                } else {
                    let query = udp_proto::Ping::decode(&incoming[1..]).expect("protobuf query");
                    query.timestamp
                };

                let mut reply = Vec::new();
                if legacy_only || incoming.len() == LEGACY_QUERY_LEN {
                    reply.extend_from_slice(&version.to_v1().to_be_bytes());
                    reply.extend_from_slice(&echo.to_be_bytes());
                    for value in [users, max, 72_000] {
                        reply.extend_from_slice(&value.to_be_bytes());
                    }
                } else {
                    reply.push(1); // UDP message type: ping
                    let answer = udp_proto::Ping {
                        timestamp: echo,
                        request_extended_information: false,
                        server_version_v2: version.to_v2(),
                        user_count: users,
                        max_user_count: max,
                        max_bandwidth_per_user: 72_000,
                    };
                    reply.extend_from_slice(&answer.encode_to_vec());
                }
                let _ = socket.send_to(&reply, from).await;
            }
        });
        addr
    }

    async fn ask(address: &str, port: u16, refresh: bool) -> QueryOutcome {
        ServerQueries::default().query(address, port, refresh).await
    }

    #[tokio::test]
    async fn a_modern_server_answers_with_the_exact_version() {
        let version = MumbleVersion::new(1, 5, 735);
        let addr = fake_server(version, 11, 20, false).await;
        let outcome = ask(&addr.ip().to_string(), addr.port(), false).await;
        let QueryOutcome::Answered(query) = outcome else {
            panic!("the fake server did not answer");
        };
        assert_eq!(query.users, 11, "users online");
        assert_eq!(query.max_users, 20, "user limit");
        assert_eq!(query.version, version);
        assert!(query.ping_ms < 1000, "round trip: {} ms", query.ping_ms);
    }

    #[tokio::test]
    async fn an_old_server_that_ignores_the_modern_query_is_asked_the_legacy_way() {
        // The legacy version field cannot hold a patch number above 255, so an old server shows
        // as 1.4.230 -> 1.4.230 (it fits) and the fallback is what makes the card work at all.
        let version = MumbleVersion::new(1, 4, 230);
        let addr = fake_server(version, 3, 100, true).await;
        let outcome = ask(&addr.ip().to_string(), addr.port(), false).await;
        let QueryOutcome::Answered(query) = outcome else {
            panic!("the legacy query got no answer");
        };
        assert_eq!(query.version, version);
        assert_eq!((query.users, query.max_users), (3, 100));
    }

    #[tokio::test]
    async fn a_server_that_stays_silent_is_unreachable() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.expect("bind");
        let addr = socket.local_addr().expect("addr");
        assert_eq!(
            ask(&addr.ip().to_string(), addr.port(), false).await,
            QueryOutcome::Unreachable
        );
    }

    #[tokio::test]
    async fn an_answer_is_reused_until_the_card_refreshes_it() {
        let queries = ServerQueries::default();
        let answer = QueryOutcome::Answered(ServerQuery {
            ping_ms: 4,
            users: 2,
            max_users: 10,
            version: MumbleVersion::new(1, 4, 2),
        });
        queries.put("127.0.0.1:1", answer.clone());
        // Nothing is sent: the stored answer is the result.
        assert_eq!(queries.query("127.0.0.1", 1, false).await, answer);
        // A refresh asks again, and nothing listens on that port.
        assert_eq!(
            queries.query("127.0.0.1", 1, true).await,
            QueryOutcome::Unreachable
        );
    }

    #[tokio::test]
    async fn a_privacy_result_is_not_stored() {
        // Whether the privacy switch is on is read from the settings on every call, so a stored
        // "not allowed" outcome must never be reused.
        let queries = ServerQueries::default();
        queries.put("example.test:64738", QueryOutcome::NotAllowed);
        assert_eq!(queries.hit("example.test:64738"), None);
    }
}
