# netsim

Local TCP+UDP forwarding proxy for tests. Simulates UDP packet loss, latency/jitter, a UDP
blackout and TCP connection kills, adjustable at runtime. Pure userspace: no root, no iptables,
identical in CI.

## Start

    cargo run -p netsim -- run --listen 127.0.0.1:64739 --target 127.0.0.1:64738 --control 127.0.0.1:64740

Defaults are exactly these values. Point the client at the `--listen` port; it is used for both
TCP and UDP (as a Mumble server does). Port 0 for `--listen` picks a free port.

## Control

Line-based TCP socket on `--control`; use `netsim ctl` or e.g. `echo status | nc 127.0.0.1 64740`.

    netsim ctl loss [up|down|both] <pct>   # UDP loss 0-100; up = client->server; default both
    netsim ctl latency <ms>                # added delay per UDP packet, both directions
    netsim ctl jitter <ms>                 # extra random delay 0..=ms (can reorder packets)
    netsim ctl block-udp on|off            # drop all UDP
    netsim ctl kill-tcp                    # cut all open TCP connections (new ones still accepted)
    netsim ctl reset                       # clear loss, latency, jitter, block
    netsim ctl status

Use `--control <addr>` on `ctl` if the proxy runs on another control address. Replies are
`ok <status>` or `err <reason>`; `ctl` exits non-zero on `err`. Set `RUST_LOG=netsim=debug`
for more logging. UDP client sessions expire after 120 s without server traffic.

## Tests

    cargo test -p netsim
