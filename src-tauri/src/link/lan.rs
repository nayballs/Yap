//! **This PC on the home network** (Yap Link): which addresses a phone can
//! reach it at — the QR's address, the certificate's names — which peers
//! may connect at all ([`allowed_peer`]: private, link-local and loopback
//! addresses only), and whether Windows marks the network a connection came
//! in on **Public** ([`public_network`]: a café's or a hotel's Wi-Fi, where
//! Yap refuses phones).
//!
//! Windows' own lists, read on this PC (IP Helper's `GetAdaptersAddresses`,
//! the Network List Manager): nothing goes on the network to find them.

use std::net::IpAddr;
use std::time::{Duration, Instant};

/// **May this peer connect?** Only addresses on a home network: private
/// IPv4 (10/8, 172.16/12, 192.168/16), link-local (169.254/16, fe80::/10),
/// unique-local IPv6 (fc00::/7) and loopback (127/8, ::1). An IPv4 address
/// written as IPv6 (`::ffff:192.168.1.5`) is judged as IPv4. Pure.
pub fn allowed_peer(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local() || v4.is_loopback(),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return allowed_peer(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            v6.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

/// One of this PC's addresses on a network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub ip: IpAddr,
    /// Windows' name for the network adapter ("Wi-Fi", "Ethernet").
    pub adapter: String,
    /// The adapter's id (its GUID, "{…}"), for the network it's on.
    pub adapter_id: String,
    /// The adapter has a default gateway: it's the way out to the router,
    /// the network a phone on the same Wi-Fi is on.
    pub gateway: bool,
    /// Wi-Fi or Ethernet (not a VPN, a virtual switch or a tunnel).
    pub physical: bool,
}

/// How good an address is for the QR, best first: on the router's network,
/// a real adapter, IPv4 (a phone types and reads it), the usual home ranges.
fn rank(a: &Address) -> (bool, bool, bool, u8) {
    let home = match a.ip {
        IpAddr::V4(v4) => match v4.octets() {
            [192, 168, ..] => 3,
            [10, ..] => 2,
            [172, b, ..] if (16..32).contains(&b) => 1,
            _ => 0,
        },
        IpAddr::V6(_) => 0,
    };
    (a.gateway, a.physical, a.ip.is_ipv4(), home)
}

/// The addresses a phone can reach, best first ([`rank`]): reachable ones
/// ([`allowed_peer`]) that aren't loopback; an IPv6 link-local one needs a
/// zone a QR can't carry, so it's left out. Pure.
pub fn reachable(mut all: Vec<Address>) -> Vec<Address> {
    all.retain(|a| allowed_peer(a.ip) && !a.ip.is_loopback() && !is_link_local_v6(a.ip));
    all.sort_by_key(|a| std::cmp::Reverse(rank(a)));
    all.dedup_by(|a, b| a.ip == b.ip);
    all
}

fn is_link_local_v6(ip: IpAddr) -> bool {
    matches!(ip, IpAddr::V6(v6) if (v6.segments()[0] & 0xffc0) == 0xfe80)
}

/// This PC's reachable addresses now, best first: the first is the QR's.
pub fn addresses() -> Vec<Address> {
    reachable(win::adapters())
}

/// This PC's name ("DESKTOP-4821"), for `ready.pc` and `<name>.local`.
pub fn pc_name() -> String {
    std::env::var("COMPUTERNAME").ok().map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| "This PC".to_string())
}

/// **Does `local` (the address a phone's connection came in on) belong to a
/// network Windows marks Public?** `None`: it can't tell (no such network
/// listed, or its list didn't answer) — then the address rule alone
/// decides. Kept 30 seconds (a burst of connections asks once).
pub fn public_network(local: IpAddr) -> Option<bool> {
    /// Each connected network's adapter ids, and whether it's Public.
    type Categories = Vec<(String, bool)>;
    if local.is_loopback() {
        return Some(false);
    }
    static CACHE: std::sync::Mutex<Option<(Instant, Categories)>> = std::sync::Mutex::new(None);
    let categories = {
        let mut c = CACHE.lock().unwrap_or_else(|p| p.into_inner());
        match c.as_ref() {
            Some((at, list)) if at.elapsed() < Duration::from_secs(30) => list.clone(),
            _ => {
                let list = win::public_adapters();
                *c = Some((Instant::now(), list.clone()));
                list
            }
        }
    };
    let adapter = win::adapters().into_iter().find(|a| a.ip == local)?.adapter_id;
    categories.iter().find(|(id, _)| id.eq_ignore_ascii_case(&adapter)).map(|(_, public)| *public)
}

#[cfg(windows)]
mod win {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
        IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows::Win32::Networking::NetworkListManager::{
        INetwork, INetworkConnection, INetworkListManager, NetworkListManager, NLM_ENUM_NETWORK_CONNECTED, NLM_NETWORK_CATEGORY_PUBLIC,
    };
    use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6};
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

    use super::Address;

    /// `IF_TYPE_ETHERNET_CSMACD`, `IF_TYPE_IEEE80211` (Wi-Fi).
    const ETHERNET: u32 = 6;
    const WIFI: u32 = 71;
    const ERROR_BUFFER_OVERFLOW: u32 = 111;

    /// An address in a `SOCKADDR`.
    ///
    /// # Safety
    /// `sa` points at a `SOCKADDR_IN` or `SOCKADDR_IN6` as its family says
    /// (what Windows hands back), or is null.
    unsafe fn ip_of(sa: *const SOCKADDR) -> Option<IpAddr> {
        if sa.is_null() {
            return None;
        }
        // SAFETY: non-null, and the family says which struct it is.
        unsafe {
            match (*sa).sa_family {
                AF_INET => {
                    let v4 = &*(sa as *const SOCKADDR_IN);
                    Some(IpAddr::V4(Ipv4Addr::from(u32::from_be(v4.sin_addr.S_un.S_addr))))
                }
                AF_INET6 => {
                    let v6 = &*(sa as *const SOCKADDR_IN6);
                    Some(IpAddr::V6(Ipv6Addr::from(v6.sin6_addr.u.Byte)))
                }
                _ => None,
            }
        }
    }

    /// Every unicast address of every adapter that's up.
    pub fn adapters() -> Vec<Address> {
        let flags = GAA_FLAG_INCLUDE_GATEWAYS | GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
        let mut size: u32 = 16 * 1024;
        let mut buf: Vec<u64> = Vec::new();
        for _ in 0..4 {
            buf = vec![0u64; (size as usize).div_ceil(8)];
            // SAFETY: the buffer is `size` bytes (rounded up), 8-aligned.
            let r = unsafe {
                GetAdaptersAddresses(u32::from(AF_UNSPEC.0), flags, None, Some(buf.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>()), &mut size)
            };
            match r {
                0 => break,
                ERROR_BUFFER_OVERFLOW => continue,
                e => {
                    tracing::info!(error = e, "Yap Link: couldn't list this PC's network adapters");
                    return Vec::new();
                }
            }
        }
        let mut out = Vec::new();
        let mut at = buf.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        // SAFETY: a linked list Windows wrote into `buf`, alive for the loop.
        unsafe {
            while !at.is_null() {
                let a = &*at;
                if a.OperStatus == IfOperStatusUp {
                    let name = a.FriendlyName.to_string().unwrap_or_default();
                    let id = a.AdapterName.to_string().unwrap_or_default();
                    let gateway = !a.FirstGatewayAddress.is_null();
                    let physical = a.IfType == ETHERNET || a.IfType == WIFI;
                    let mut u = a.FirstUnicastAddress;
                    while !u.is_null() {
                        if let Some(ip) = ip_of((*u).Address.lpSockaddr) {
                            out.push(Address { ip, adapter: name.clone(), adapter_id: id.clone(), gateway, physical });
                        }
                        u = (*u).Next;
                    }
                }
                at = a.Next;
            }
        }
        out
    }

    fn guid_text(g: &windows::core::GUID) -> String {
        let d = g.data4;
        format!(
            "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
            g.data1, g.data2, g.data3, d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]
        )
    }

    /// The connected networks' adapters, each with whether its network is
    /// Public. Empty when the list can't be read.
    pub fn public_adapters() -> Vec<(String, bool)> {
        let read = || -> windows::core::Result<Vec<(String, bool)>> {
            let mut out = Vec::new();
            // SAFETY: COM calls on interfaces Windows hands back; the
            // process keeps a multithreaded apartment up (`shell/com.rs`).
            unsafe {
                let nlm: INetworkListManager = CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL)?;
                let networks = nlm.GetNetworks(NLM_ENUM_NETWORK_CONNECTED)?;
                loop {
                    let mut one: [Option<INetwork>; 1] = [None];
                    let mut got = 0u32;
                    networks.Next(&mut one, Some(&mut got))?;
                    let Some(network) = one[0].take().filter(|_| got > 0) else { break };
                    let public = network.GetCategory()? == NLM_NETWORK_CATEGORY_PUBLIC;
                    let connections = network.GetNetworkConnections()?;
                    loop {
                        let mut c: [Option<INetworkConnection>; 1] = [None];
                        let mut n = 0u32;
                        connections.Next(&mut c, Some(&mut n))?;
                        let Some(conn) = c[0].take().filter(|_| n > 0) else { break };
                        if let Ok(g) = conn.GetAdapterId() {
                            out.push((guid_text(&g), public));
                        }
                    }
                }
            }
            Ok(out)
        };
        read().unwrap_or_else(|e| {
            tracing::info!("Yap Link: couldn't read which networks are Public ({})", e.message());
            Vec::new()
        })
    }
}

#[cfg(not(windows))]
mod win {
    pub fn adapters() -> Vec<super::Address> {
        Vec::new()
    }
    pub fn public_adapters() -> Vec<(String, bool)> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    /// Windows' own lists on this PC (run by hand: `cargo test link::lan --
    /// --ignored --nocapture`): its adapters' addresses and which networks
    /// are Public, read without an error. Prints counts only.
    #[test]
    #[ignore = "reads this PC's networks"]
    fn reads_this_pcs_networks() {
        crate::shell::com::keep_mta();
        let all = win::adapters();
        let categories = win::public_adapters();
        let reachable = reachable(all.clone());
        println!("adapters' addresses: {}, reachable: {}, connected networks' adapters: {}", all.len(), reachable.len(), categories.len());
        if let Some(best) = reachable.first() {
            println!("the QR's address is on a network Windows marks Public: {:?}", public_network(best.ip));
        }
        assert!(!all.is_empty(), "loopback at least");
    }

    #[test]
    fn only_peers_on_a_home_network_may_connect() {
        for ok in ["10.0.0.5", "10.255.255.255", "172.16.0.1", "172.31.9.9", "192.168.1.20", "169.254.3.4", "127.0.0.1", "127.8.8.8", "::1", "fd12:3456::1", "fc00::1", "fe80::1", "::ffff:192.168.1.5"] {
            assert!(allowed_peer(ip(ok)), "{ok}");
        }
        for no in ["8.8.8.8", "172.15.0.1", "172.32.0.1", "100.64.0.1", "192.169.0.1", "1.1.1.1", "2001:db8::1", "2606:4700::1111", "::ffff:8.8.8.8", "0.0.0.0", "255.255.255.255"] {
            assert!(!allowed_peer(ip(no)), "{no}");
        }
    }

    fn addr(s: &str, gateway: bool, physical: bool) -> Address {
        Address { ip: ip(s), adapter: String::new(), adapter_id: String::new(), gateway, physical }
    }

    #[test]
    fn the_qr_names_the_address_on_the_routers_network() {
        let list = reachable(vec![
            addr("127.0.0.1", false, false),
            addr("172.20.112.1", false, false), // WSL's virtual switch
            addr("fe80::1", true, true),         // needs a zone
            addr("8.8.4.4", true, true),         // not a home address
            addr("10.8.0.2", true, false),        // a VPN with a gateway
            addr("192.168.1.20", true, true),     // the Wi-Fi
            addr("fd00::20", true, true),
        ]);
        let ips: Vec<String> = list.iter().map(|a| a.ip.to_string()).collect();
        assert_eq!(ips, ["192.168.1.20", "fd00::20", "10.8.0.2", "172.20.112.1"]);
        assert!(reachable(Vec::new()).is_empty());
    }
}
