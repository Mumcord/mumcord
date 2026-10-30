//! The public server query behind the `mumble://` link card (issue #175).
//!
//! A client can ask a Mumble server for its version, the users online and its user limit without
//! connecting: the UDP ping that requests "extended information". Servers since 1.2.3 answer it
//! when pings are allowed (murmur's default `allowPing=true`), and a server answers in the format
//! the request was written in. Two formats are therefore used:
//!
//! - Protobuf (servers from 1.5): reports the version exactly.
//! - Legacy (servers up to 1.4): the only format older servers know, and its version field cannot
//!   hold a patch number above 255.
//!
//! The engine asks in the modern format and falls back to the legacy one, so old servers answer
//! too. Everything that comes back is untrusted input.

use prost::Message;

use crate::udp_proto;
use crate::version::MumbleVersion;

/// Header byte of a protobuf UDP ping.
const PROTO_PING: u8 = 1;
/// Length of the legacy query: four zero bytes plus the value the server echoes.
pub const LEGACY_QUERY_LEN: usize = 12;
/// Length of the legacy answer (no header, five big-endian words plus the echoed value).
pub const LEGACY_ANSWER_LEN: usize = 24;

/// The query in the modern format. `echo` is a value the server sends back unchanged, which is
/// how the answer is matched to the request.
pub fn encode_query(echo: u64) -> Vec<u8> {
    let message = udp_proto::Ping {
        timestamp: echo,
        request_extended_information: true,
        ..Default::default()
    };
    let mut out = vec![PROTO_PING];
    out.extend_from_slice(&message.encode_to_vec());
    out
}

/// The query in the legacy format: the four zero bytes are the slot the server writes its version
/// into, the eight following bytes carry `echo`.
pub fn encode_legacy_query(echo: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(LEGACY_QUERY_LEN);
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&echo.to_be_bytes());
    out
}

/// What a server answered. `echo` is the value the query carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryAnswer {
    pub version: MumbleVersion,
    pub users: u32,
    pub max_users: u32,
    pub echo: u64,
}

/// Reads an answer in either format; `None` when the packet is not a query answer (wrong length,
/// undecodable message, or a plain connectivity ping without the figures).
pub fn decode_answer(data: &[u8]) -> Option<QueryAnswer> {
    if data.len() == LEGACY_ANSWER_LEN {
        let word = |i: usize| u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
        // An all-zero version means the server did not fill the field in.
        let version = MumbleVersion::from_v1(word(0));
        if version == MumbleVersion::new(0, 0, 0) {
            return None;
        }
        return Some(QueryAnswer {
            version,
            users: word(12),
            max_users: word(16),
            echo: u64::from_be_bytes(data[4..12].try_into().ok()?),
        });
    }

    let (&header, body) = data.split_first()?;
    if header != PROTO_PING {
        return None;
    }
    let message = udp_proto::Ping::decode(body).ok()?;
    // A plain connectivity ping carries no figures; 0 means the field was left out.
    if message.server_version_v2 == 0 {
        return None;
    }
    Some(QueryAnswer {
        version: MumbleVersion::from_v2(message.server_version_v2),
        users: message.user_count,
        max_users: message.max_user_count,
        echo: message.timestamp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_answer(version: MumbleVersion, users: u32, max: u32, echo: u64) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&version.to_v1().to_be_bytes());
        data.extend_from_slice(&echo.to_be_bytes());
        for value in [users, max, 72_000] {
            data.extend_from_slice(&value.to_be_bytes());
        }
        data
    }

    #[test]
    fn legacy_query_is_the_extended_ping() {
        let query = encode_legacy_query(0x1122_3344_5566_7788);
        assert_eq!(query.len(), LEGACY_QUERY_LEN);
        assert_eq!(&query[..4], &[0, 0, 0, 0]);
        assert_eq!(
            &query[4..],
            &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]
        );
    }

    #[test]
    fn protobuf_query_round_trips() {
        let query = encode_query(0xDEAD_BEEF);
        assert_eq!(query[0], PROTO_PING);
        assert_eq!(
            decode_answer(&query),
            None,
            "the query itself is not an answer"
        );
        // A server that answers the query puts the figures into the same message.
        let mut answer = udp_proto::Ping {
            timestamp: 0xDEAD_BEEF,
            request_extended_information: true,
            server_version_v2: MumbleVersion::new(1, 5, 735).to_v2(),
            user_count: 7,
            max_user_count: 50,
            max_bandwidth_per_user: 72_000,
        };
        answer.request_extended_information = false;
        let mut data = vec![PROTO_PING];
        data.extend_from_slice(&answer.encode_to_vec());
        assert_eq!(
            decode_answer(&data),
            Some(QueryAnswer {
                version: MumbleVersion::new(1, 5, 735),
                users: 7,
                max_users: 50,
                echo: 0xDEAD_BEEF
            })
        );
    }

    #[test]
    fn reads_the_legacy_answer() {
        let v = MumbleVersion::new(1, 4, 230);
        assert_eq!(
            decode_answer(&legacy_answer(v, 3, 100, 0xDEAD_BEEF)),
            Some(QueryAnswer {
                version: v,
                users: 3,
                max_users: 100,
                echo: 0xDEAD_BEEF
            })
        );
    }

    #[test]
    fn ignores_answers_that_are_not_figures() {
        assert_eq!(decode_answer(&[0x20, 0x05]), None);
        assert_eq!(decode_answer(&[]), None);
        assert_eq!(decode_answer(&[0; LEGACY_ANSWER_LEN]), None);
    }
}
