use serde::{Deserialize, Serialize};

use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Board {
    title: String,
    round: u32,
    delta: i64,
    speed: f64,
    done: bool,
    note: Option<String>,
    tags: Vec<String>,
    grid: Vec<Vec<u8>>,
    players: Vec<Player>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Player {
    name: String,
    score: i32,
}

impl Message for Board {
    fn version() -> u32 {
        3
    }
}

fn board() -> Board {
    Board {
        title: "Grüße \"quoted\"\ttab".into(),
        round: 7,
        delta: -42,
        speed: 2.0,
        done: true,
        note: None,
        tags: vec!["a".into(), "b c".into()],
        grid: vec![vec![1, 2], vec![3, 4]],
        players: vec![
            Player {
                name: "Ann".into(),
                score: -1,
            },
            Player {
                name: "Bo".into(),
                score: 9,
            },
        ],
    }
}

#[test]
fn round_trips() {
    let bytes = encode(&board(), RELAY_LIMIT).expect("encodes");
    assert!(bytes.len() <= MUMBLE_LIMIT, "{} bytes", bytes.len());
    let got = decode::<Board>(&bytes).expect("decodes");
    assert_eq!(
        got,
        Received {
            version: 3,
            message: board()
        }
    );
}

#[test]
fn unknown_fields_are_ignored() {
    #[derive(Serialize, Deserialize)]
    struct Newer {
        round: u32,
        extra: Vec<f64>,
    }
    impl Message for Newer {
        fn version() -> u32 {
            4
        }
    }
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Older {
        round: u32,
    }
    impl Message for Older {
        fn version() -> u32 {
            1
        }
    }
    let bytes = encode(
        &Newer {
            round: 5,
            extra: vec![0.5],
        },
        MUMBLE_LIMIT,
    )
    .expect("encodes");
    let got = decode::<Older>(&bytes).expect("decodes");
    assert_eq!(
        got,
        Received {
            version: 4,
            message: Older { round: 5 }
        }
    );
}

#[test]
fn limits_are_enforced() {
    let mut big = board();
    big.tags = vec!["x".repeat(100); 20];
    assert_eq!(encode(&big, MUMBLE_LIMIT), Err(Error::TooLarge));
    assert!(encode(&big, RELAY_LIMIT).is_ok());
    big.title = "y".repeat(2000);
    assert_eq!(encode(&big, MUMBLE_LIMIT), Err(Error::TooLarge));
    assert_eq!(
        decode::<Board>(&vec![b' '; RELAY_LIMIT + 1]),
        Err(Error::TooLarge)
    );
}

#[test]
fn bad_documents_are_refused() {
    let bytes = encode(&board(), RELAY_LIMIT).expect("encodes");
    assert_eq!(
        decode::<Board>(&bytes[..bytes.len() / 2]),
        Err(Error::Malformed)
    );
    assert_eq!(decode::<Board>(b"\xff\x00garbage{{"), Err(Error::Malformed));
    assert_eq!(decode::<Board>(b""), Err(Error::WrongType));

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Other {
        round: String,
    }
    impl Message for Other {
        fn version() -> u32 {
            1
        }
    }
    assert_eq!(decode::<Other>(&bytes), Err(Error::WrongType));
}

#[test]
fn nesting_is_capped() {
    #[derive(Serialize, Deserialize)]
    struct Deep(serde_json::Value);
    impl Message for Deep {
        fn version() -> u32 {
            1
        }
    }
    let mut v = serde_json::json!(1);
    for _ in 0..MAX_DEPTH {
        v = serde_json::json!([v]);
    }
    assert!(encode(&Deep(v.clone()), RELAY_LIMIT).is_ok());
    assert_eq!(
        encode(&Deep(serde_json::json!([v])), RELAY_LIMIT),
        Err(Error::Malformed)
    );
}

#[test]
fn values_bovnar_cannot_carry_are_refused() {
    #[derive(Serialize, Deserialize)]
    struct Raw(serde_json::Value);
    impl Message for Raw {
        fn version() -> u32 {
            1
        }
    }
    let bad_key = serde_json::json!({ "1st": 1 });
    assert_eq!(encode(&Raw(bad_key), RELAY_LIMIT), Err(Error::Malformed));
    let control = serde_json::json!({ "s": "a\u{1}b" });
    assert_eq!(encode(&Raw(control), RELAY_LIMIT), Err(Error::Malformed));
}

#[test]
fn lists_of_lists_must_be_rectangular() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Rows(Vec<Vec<i32>>);
    impl Message for Rows {
        fn version() -> u32 {
            1
        }
    }
    // Bovnar lists of lists are rectangular.
    assert_eq!(
        encode(&Rows(vec![vec![1, -2], vec![3]]), RELAY_LIMIT),
        Err(Error::Malformed)
    );
    let square = Rows(vec![vec![1, -2], vec![3, 4]]);
    let bytes = encode(&square, RELAY_LIMIT).expect("encodes");
    assert_eq!(decode::<Rows>(&bytes).expect("decodes").message, square);
}
