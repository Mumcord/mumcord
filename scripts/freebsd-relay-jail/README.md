# BastilleBSD template: Mumble test server + mumpeer relay + mumbot peer bot in one jail

A throw-away jail for testing Mumcord's peer-to-peer features (file sharing, screen sharing,
cameras) on FreeBSD, the same way `mumbot/examples/peer-with-relay` does it with Docker. It is
test tooling for the FreeBSD port, not a production deployment: the server has no registration
rules, the relay runs on a generated self-signed identity and everything runs as root inside the
jail. Production relay and bot deployments belong to their own repositories
([mumpeer](https://github.com/Mumcord/mumpeer),
[mumbot](https://github.com/Mumcord/mumbot)).

## What you get

| Service                                  | Address                                | Notes                                  |
| ---------------------------------------- | -------------------------------------- | -------------------------------------- |
| Mumble server (mumble-server from ports) | `mumble://<jail_ip>:64738`             | unmodified FreeBSD package             |
| mumpeer relay                            | `443/udp` and `443/tcp` on `<jail_ip>` | built from source in the jail          |
| mumbot peer bot                          | joins the server as `RelayBot`         | announces the relay to Mumcord clients |

## Usage

On a FreeBSD host with Bastille installed:

```sh
bastille bootstrap 15.1-RELEASE
ifconfig em0 alias 192.0.2.11/24          # any free LAN address
bastille create relay 15.1-RELEASE 192.0.2.11 em0
bastille template relay "$PWD/scripts/freebsd-relay-jail" \
    --arg jail_ip=192.0.2.11 --arg relay_host=192.0.2.11
```

The template installs the packages, builds both services inside the jail (a few minutes on a
small box), starts the Mumble server, starts the relay, pins both certificates into the bot
config, registers the bot and starts it. The last line of the output is the address to point
Mumcord at.

To add the alias permanently, put `ifconfig_em0_alias0=inet 192.0.2.11/24` into
`/etc/rc.conf`.

## Managing the stack

```sh
bastille cmd relay sh /root/setup-stack.sh          # (re)start everything, re-pin
bastille cmd relay service mumble_server stop       # Mumble server
bastille cmd relay pkill -f /root/bin/mumpeer       # relay
bastille cmd relay pkill -f 'mumbot peer'           # bot
bastille cmd relay tail -f /var/log/mumpeer/mumpeer.log
bastille cmd relay tail -f /var/log/mumpeer/bot.log
```

## FreeBSD details this template handles

- **No IPv6 in a shared-IP jail.** Both the Mumble server and the relay default to a dual-stack
  `[::]` listener and fail with "Protocol type not supported". The configs bind IPv4 only
  (`host=` for murmur, `udp_listen`/`tcp_listen` for the relay).
- **No loopback in a shared-IP jail.** `127.0.0.1` is the host's loopback, so the relay's
  allowlist API listens on the jail address; it then uses HTTPS with the relay's self-signed
  certificate and the bot pins that certificate (`api_fp`).
- **mumbot needs a `protoc`.** Its `mumcord-protocol` dependency cannot find a vendored protoc on
  FreeBSD; `build-stack.sh` installs the port and patches the crate to use it (a `[patch.crates-io]`
  path patch, so cargo really rebuilds it — an edited vendored crate is treated as unchanged).
- **The bot must be registered.** Mumcord only trusts a relay announced by a registered user, so
  the setup script self-registers `RelayBot` before starting the bot.
- **The jail needs a resolver that answers its own address.** A shared-IP jail sends its DNS
  queries from the jail address, and a home router often only answers queries from the host. If
  `pkg` and `git` fail with "Transient resolver failure" inside the jail, run a small forwarder on
  the host for that address:

  ```sh
  pkg install dnsmasq
  printf 'port=53\nlisten-address=192.0.2.11\nbind-interfaces\nno-resolv\nserver=192.0.2.1\ncache-size=1000\n' \
      > /usr/local/etc/dnsmasq-relay.conf
  daemon /usr/local/sbin/dnsmasq -C /usr/local/etc/dnsmasq-relay.conf
  printf 'search example.org\nnameserver 192.0.2.11\n' > \
      /usr/local/bastille/jails/relay/root/etc/resolv.conf
  ```

## Caveats of the test rig

- `registered_only = false` in `bot.toml` lets guests use the relay, so a fresh Mumcord profile
  works without registering it on the server. Do not do that in production.
- Both services run as root inside the jail and the relay's data directory is a plain directory,
  not a volume. Delete the jail when you are done.
- The Mumble server's SuperUser password is not set; use `mumble-server -supw <password>` inside
  the jail if you need it.
