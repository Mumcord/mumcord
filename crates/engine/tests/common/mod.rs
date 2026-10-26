//! Ports of the testenv instance under test. Same scheme as `testenv/env.sh`: `MUMCORD_PORT_BASE`
//! is the port of v15 (default 64738); v14, v13, limit and v15b follow at +1 to +4.
#![allow(dead_code)]

fn base() -> u16 {
    std::env::var("MUMCORD_PORT_BASE")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(64738)
}

pub fn v15() -> u16 {
    base()
}

pub fn v14() -> u16 {
    base() + 1
}

pub fn v13() -> u16 {
    base() + 2
}

pub fn limit() -> u16 {
    base() + 3
}

pub fn v15b() -> u16 {
    base() + 4
}
