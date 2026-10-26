//! Where this client can be reached: addresses of the network interfaces (IPv6, LAN, a public
//! IPv4 on the machine) and, if the home router allows it, a port mapping through PCP/NAT-PMP
//! or UPnP. No STUN or other outside server is asked (QA.90).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4};
use std::num::NonZeroU16;
use std::time::Duration;

use crab_nat::{InternetProtocol, PortMapping, PortMappingOptions, PortMappingType, TimeoutConfig};
use igd_next::PortMappingProtocol;
use igd_next::aio::Gateway;
use igd_next::aio::tokio::Tokio;

use crate::signal::MAX_CANDIDATES;

/// Requested lifetime of a router port mapping. An active UDP flow keeps the router's state
/// after the lease ends, so a connection outlives it; new connections request a fresh mapping.
const MAPPING_LEASE_SECS: u32 = 3600;
/// Time budget for asking the router, so an offer is not held up by a router that never answers.
const MAPPING_BUDGET: Duration = Duration::from_secs(3);
const MAPPING_DESCRIPTION: &str = "Mumcord direct connection";

/// A router port mapping; removed again when dropped.
pub(crate) struct MappingGuard(Option<Mapping>);

enum Mapping {
    Nat(PortMapping),
    Upnp { gateway: Gateway<Tokio>, port: u16 },
}

impl Drop for MappingGuard {
    fn drop(&mut self) {
        let Some(mapping) = self.0.take() else {
            return;
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::warn!("no runtime to remove the router port mapping; it expires on its own");
            return;
        };
        runtime.spawn(async move {
            let result = match mapping {
                Mapping::Nat(m) => m.try_drop().await.map_err(|(e, _)| e.to_string()),
                Mapping::Upnp { gateway, port } => gateway
                    .remove_port(PortMappingProtocol::UDP, port)
                    .await
                    .map_err(|e| e.to_string()),
            };
            if let Err(e) = result {
                tracing::warn!("removing the router port mapping failed: {e}");
            }
        });
    }
}

/// Candidates for the socket bound to `port`, most likely to work first, at most
/// [`MAX_CANDIDATES`]. The mapping (if any) must live as long as the connection.
pub(crate) async fn candidates(
    port: u16,
    port_mapping: bool,
    loopback: bool,
) -> (Vec<SocketAddr>, Option<MappingGuard>) {
    let mut list = Vec::new();
    // Only tests ask for loopback; first, so the cap never drops it.
    if loopback {
        list.push(SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port));
    }
    let mut guard = None;
    if port_mapping {
        match tokio::time::timeout(MAPPING_BUDGET, map_port(port)).await {
            Ok(Some((external, mapping))) => {
                list.push(external);
                guard = Some(MappingGuard(Some(mapping)));
            }
            Ok(None) => tracing::info!("router offers no port mapping"),
            Err(_) => tracing::info!("router port mapping timed out"),
        }
    }
    list.extend(
        interface_addresses()
            .into_iter()
            .map(|ip| SocketAddr::new(ip, port)),
    );
    let mut seen = std::collections::HashSet::new();
    list.retain(|a| seen.insert(*a));
    list.truncate(MAX_CANDIDATES);
    (list, guard)
}

/// Most IPv6 addresses offered: privacy/temporary addresses can be numerous and would otherwise
/// crowd the LAN IPv4 out of [`MAX_CANDIDATES`].
const MAX_IPV6: usize = 3;

/// Name prefixes of virtual and bridge interfaces (Docker, libvirt, VMware, VirtualBox, LXC,
/// Podman, CNI). Their private addresses are useless to peers and leak the network layout. VPN
/// interfaces (wg*, tun*, ...) are deliberately kept: users may want Mumcord over a VPN.
const VIRTUAL_PREFIXES: [&str; 9] = [
    "docker", "br-", "veth", "virbr", "vmnet", "vboxnet", "lxcbr", "podman", "cni",
];

/// The parts of a network interface that candidate selection looks at.
struct Iface {
    name: String,
    up: bool,
    loopback: bool,
    v4: Vec<Ipv4Addr>,
    v6: Vec<Ipv6Addr>,
}

fn is_virtual(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    VIRTUAL_PREFIXES.iter().any(|p| name.starts_with(p))
}

/// Interface addresses another machine could reach: global IPv6, unique-local IPv6 (at most
/// [`MAX_IPV6`] together), then IPv4 with the default-route address first. Link-local, loopback
/// and virtual interfaces are left out. The order does not depend on the interface order.
fn interface_addresses() -> Vec<IpAddr> {
    let ifaces: Vec<Iface> = netdev::get_interfaces()
        .into_iter()
        .map(|i| Iface {
            up: i.is_up(),
            loopback: i.is_loopback(),
            v4: i.ipv4.iter().map(|n| n.addr()).collect(),
            v6: i.ipv6.iter().map(|n| n.addr()).collect(),
            name: i.name,
        })
        .collect();
    select_addresses(&ifaces, default_route().map(|(_, local)| local))
}

fn select_addresses(ifaces: &[Iface], primary_v4: Option<Ipv4Addr>) -> Vec<IpAddr> {
    let mut v6 = Vec::new();
    let mut v4 = Vec::new();
    for iface in ifaces {
        if !iface.up || iface.loopback || is_virtual(&iface.name) {
            continue;
        }
        for &ip in &iface.v6 {
            if !(ip.is_loopback() || ip.is_unspecified() || ip.is_unicast_link_local()) {
                // Global addresses before unique-local ones (fc00::/7).
                let local = (ip.segments()[0] & 0xfe00) == 0xfc00;
                v6.push((local, ip));
            }
        }
        for &ip in &iface.v4 {
            if !(ip.is_loopback() || ip.is_unspecified() || ip.is_link_local()) {
                v4.push(ip);
            }
        }
    }
    v6.sort();
    v6.dedup();
    v4.sort_by_key(|ip| (Some(*ip) != primary_v4, *ip));
    v4.dedup();
    v6.into_iter()
        .take(MAX_IPV6)
        .map(|(_, ip)| IpAddr::V6(ip))
        .chain(v4.into_iter().map(IpAddr::V4))
        .collect()
}

/// Maps the UDP `port` on the home router: PCP, then NAT-PMP, then UPnP. Returns the public
/// address and the mapping, or `None` when the router does not offer any of them.
async fn map_port(port: u16) -> Option<(SocketAddr, Mapping)> {
    let internal = NonZeroU16::new(port)?;
    let (gateway, local) = default_route()?;
    match map_nat(gateway, local, internal).await {
        Ok(found) => return Some(found),
        Err(e) => tracing::debug!("PCP/NAT-PMP port mapping failed: {e}"),
    }
    match map_upnp(local, port).await {
        Ok(found) => Some(found),
        Err(e) => {
            tracing::debug!("UPnP port mapping failed: {e}");
            None
        }
    }
}

/// The IPv4 default gateway and this machine's address towards it.
fn default_route() -> Option<(Ipv4Addr, Ipv4Addr)> {
    let iface = netdev::get_default_interface().ok()?;
    let gateway = *iface.gateway.as_ref()?.ipv4.first()?;
    let local = iface.ipv4.first()?.addr();
    Some((gateway, local))
}

async fn map_nat(
    gateway: Ipv4Addr,
    local: Ipv4Addr,
    internal: NonZeroU16,
) -> Result<(SocketAddr, Mapping), String> {
    let timeout_config = TimeoutConfig {
        initial_timeout: Duration::from_millis(250),
        max_retries: 2,
        max_retry_timeout: Some(Duration::from_secs(1)),
    };
    let mapping = PortMapping::new(
        gateway.into(),
        local.into(),
        InternetProtocol::Udp,
        internal,
        PortMappingOptions {
            external_port: Some(internal),
            lifetime_seconds: Some(MAPPING_LEASE_SECS),
            timeout_config: Some(timeout_config),
        },
    )
    .await
    .map_err(|e| format!("{e:?}"))?;
    let external_ip = match mapping.mapping_type() {
        PortMappingType::Pcp { external_ip, .. } => external_ip,
        PortMappingType::NatPmp => {
            crab_nat::natpmp::external_address(gateway.into(), Some(timeout_config))
                .await
                .map_err(|e| format!("{e:?}"))?
                .into()
        }
    };
    let external = SocketAddr::new(external_ip, mapping.external_port().get());
    let guard = Mapping::Nat(mapping);
    public(external).map(|a| (a, guard)).ok_or_else(not_public)
}

async fn map_upnp(local: Ipv4Addr, port: u16) -> Result<(SocketAddr, Mapping), String> {
    let mut options = igd_next::SearchOptions::default();
    options.timeout = Some(Duration::from_secs(2));
    options.single_search_timeout = Some(Duration::from_secs(1));
    let gateway = igd_next::aio::tokio::search_gateway(options)
        .await
        .map_err(|e| e.to_string())?;
    gateway
        .add_port(
            PortMappingProtocol::UDP,
            port,
            SocketAddrV4::new(local, port).into(),
            MAPPING_LEASE_SECS,
            MAPPING_DESCRIPTION,
        )
        .await
        .map_err(|e| e.to_string())?;
    let mapping = Mapping::Upnp {
        gateway: gateway.clone(),
        port,
    };
    let external_ip = gateway.get_external_ip().await.map_err(|e| e.to_string())?;
    public(SocketAddr::new(external_ip, port))
        .map(|a| (a, mapping))
        .ok_or_else(not_public)
}

/// A router behind another NAT (double NAT, carrier-grade NAT) reports a private address that
/// nobody outside can reach, so it is no candidate.
fn public(addr: SocketAddr) -> Option<SocketAddr> {
    match addr.ip() {
        IpAddr::V4(ip) => {
            let shared = ip.octets()[0] == 100 && (ip.octets()[1] & 0xc0) == 64;
            let private = ip.is_private() || ip.is_loopback() || ip.is_link_local() || shared;
            (!private && !ip.is_unspecified()).then_some(addr)
        }
        IpAddr::V6(_) => Some(addr),
    }
}

fn not_public() -> String {
    "router reports a non-public address (double or carrier-grade NAT)".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iface(name: &str, v4: &[&str], v6: &[&str]) -> Iface {
        Iface {
            name: name.to_owned(),
            up: true,
            loopback: false,
            v4: v4.iter().map(|a| a.parse().expect("v4")).collect(),
            v6: v6.iter().map(|a| a.parse().expect("v6")).collect(),
        }
    }

    fn ips(list: &[IpAddr]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn virtual_interfaces_are_skipped_but_vpn_is_kept() {
        let list = select_addresses(
            &[
                iface("docker0", &["172.17.0.1"], &[]),
                iface("br-1a2b", &["172.18.0.1"], &[]),
                iface("virbr0", &["192.168.122.1"], &[]),
                iface("wg0", &["10.8.0.2"], &[]),
                iface("eth0", &["192.168.1.5"], &[]),
            ],
            Some("192.168.1.5".parse().expect("v4")),
        );
        assert_eq!(ips(&list), ["192.168.1.5", "10.8.0.2"]);
    }

    #[test]
    fn many_ipv6_keep_lan_ipv4() {
        let v6: Vec<String> = (1..=10).map(|n| format!("2001:db8::{n}")).collect();
        let v6: Vec<&str> = v6.iter().map(String::as_str).collect();
        let list = select_addresses(&[iface("eth0", &["192.168.1.5"], &v6)], None);
        assert_eq!(list.len(), MAX_IPV6 + 1);
        assert_eq!(
            list.last().map(ToString::to_string).as_deref(),
            Some("192.168.1.5")
        );
        assert!(list.len() < MAX_CANDIDATES);
    }

    #[test]
    fn order_is_global_v6_then_unique_local_then_primary_v4_first() {
        let iface_a = || {
            iface(
                "eth0",
                &["10.0.0.9", "192.168.1.5"],
                &["fd00::1", "2001:db8::2"],
            )
        };
        let iface_b = || iface("wlan0", &["10.0.0.1"], &["2001:db8::1", "fe80::1"]);
        let primary = Some("192.168.1.5".parse().expect("v4"));
        let expected = [
            "2001:db8::1",
            "2001:db8::2",
            "fd00::1",
            "192.168.1.5",
            "10.0.0.1",
            "10.0.0.9",
        ];
        assert_eq!(
            ips(&select_addresses(&[iface_a(), iface_b()], primary)),
            expected
        );
        assert_eq!(
            ips(&select_addresses(&[iface_b(), iface_a()], primary)),
            expected
        );
    }

    #[test]
    fn private_router_addresses_are_no_candidates() {
        for ip in [
            "10.1.2.3",
            "192.168.0.1",
            "100.64.0.1",
            "100.127.255.1",
            "0.0.0.0",
        ] {
            assert_eq!(
                public(SocketAddr::new(ip.parse().expect("addr"), 1)),
                None,
                "{ip}"
            );
        }
        for ip in ["203.0.113.9", "100.128.0.1"] {
            assert!(
                public(SocketAddr::new(ip.parse().expect("addr"), 1)).is_some(),
                "{ip}"
            );
        }
    }

    #[tokio::test]
    async fn loopback_candidate_only_on_request() {
        let (with, _) = candidates(4000, false, true).await;
        assert!(with.contains(&"127.0.0.1:4000".parse().expect("addr")));
        let (without, _) = candidates(4000, false, false).await;
        assert!(without.iter().all(|a| !a.ip().is_loopback()));
        assert!(without.len() <= MAX_CANDIDATES);
    }
}
