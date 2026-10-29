// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Addresses, routes, DNS and counters of a tunnel adapter, through the IP
//! Helper API (the same calls `wireguard-windows` makes).
//!
//! Addresses, on-link routes and DNS belong to the adapter and disappear with
//! it. The host routes that keep the server reachable outside a full tunnel
//! belong to the physical interface: [`Routes`] remembers and removes them.
//!
//! NOT YET RUN ON WINDOWS (docs/WINDOWS.md).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use windows_sys::Win32::Foundation::NO_ERROR;
use windows_sys::Win32::NetworkManagement::IpHelper::{
    ConvertInterfaceLuidToGuid, CreateIpForwardEntry2, CreateUnicastIpAddressEntry,
    DNS_INTERFACE_SETTINGS, DNS_INTERFACE_SETTINGS_VERSION1, DNS_SETTING_NAMESERVER,
    DNS_SETTING_SEARCHLIST, DeleteIpForwardEntry2, GetBestRoute2, GetIfEntry2,
    InitializeIpForwardEntry, InitializeUnicastIpAddressEntry, MIB_IF_ROW2, MIB_IPFORWARD_ROW2,
    MIB_UNICASTIPADDRESS_ROW, SetInterfaceDnsSettings,
};
use windows_sys::Win32::NetworkManagement::Ndis::NET_LUID_LH;
use windows_sys::Win32::Networking::WinSock::{
    AF_INET, AF_INET6, IN_ADDR, IN_ADDR_0, IN6_ADDR, IN6_ADDR_0, IpDadStatePreferred, SOCKADDR_IN,
    SOCKADDR_IN6, SOCKADDR_INET,
};
use windows_sys::core::GUID;

use tunnelbird_core::config::IpNet;
use tunnelbird_core::protocol::Counters;

use super::wide;

pub fn in_addr(ip: Ipv4Addr) -> IN_ADDR {
    IN_ADDR {
        S_un: IN_ADDR_0 {
            S_addr: u32::from_ne_bytes(ip.octets()),
        },
    }
}

pub fn in6_addr(ip: Ipv6Addr) -> IN6_ADDR {
    IN6_ADDR {
        u: IN6_ADDR_0 { Byte: ip.octets() },
    }
}

pub fn sockaddr(addr: SocketAddr) -> SOCKADDR_INET {
    // SAFETY: plain data; every field set below or left zero.
    let mut out: SOCKADDR_INET = unsafe { std::mem::zeroed() };
    match addr {
        SocketAddr::V4(v4) => {
            out.Ipv4 = SOCKADDR_IN {
                sin_family: AF_INET,
                sin_port: v4.port().to_be(),
                sin_addr: in_addr(*v4.ip()),
                sin_zero: [0; 8],
            };
        }
        SocketAddr::V6(v6) => {
            // SAFETY: zero is a valid SOCKADDR_IN6 (scope and flow info 0).
            let mut six: SOCKADDR_IN6 = unsafe { std::mem::zeroed() };
            six.sin6_family = AF_INET6;
            six.sin6_port = v6.port().to_be();
            six.sin6_addr = in6_addr(*v6.ip());
            out.Ipv6 = six;
        }
    }
    out
}

fn ip_sockaddr(ip: IpAddr) -> SOCKADDR_INET {
    sockaddr(SocketAddr::new(ip, 0))
}

/// Gives the adapter its addresses inside the VPN.
pub fn add_addresses(luid: NET_LUID_LH, addresses: &[IpNet]) -> Result<(), String> {
    for net in addresses {
        // SAFETY: the row is initialised by the API, then filled.
        let mut row: MIB_UNICASTIPADDRESS_ROW = unsafe { std::mem::zeroed() };
        unsafe { InitializeUnicastIpAddressEntry(&mut row) };
        row.InterfaceLuid = luid;
        row.Address = ip_sockaddr(net.addr);
        row.OnLinkPrefixLength = net.prefix;
        row.DadState = IpDadStatePreferred;
        // SAFETY: fully initialised row.
        let status = unsafe { CreateUnicastIpAddressEntry(&row) };
        if status != NO_ERROR {
            return Err(format!(
                "Cannot add the address {}/{} (error {status})",
                net.addr, net.prefix
            ));
        }
    }
    Ok(())
}

/// Routes `nets` through the adapter (on-link, no gateway).
pub fn add_tunnel_routes(luid: NET_LUID_LH, nets: &[IpNet]) -> Result<(), String> {
    for net in nets {
        let row = route_row(luid, net, None);
        // SAFETY: fully initialised row.
        let status = unsafe { CreateIpForwardEntry2(&row) };
        if status != NO_ERROR {
            return Err(format!(
                "Cannot add the route {}/{} (error {status})",
                net.addr, net.prefix
            ));
        }
    }
    Ok(())
}

fn route_row(
    luid: NET_LUID_LH,
    net: &IpNet,
    next_hop: Option<SOCKADDR_INET>,
) -> MIB_IPFORWARD_ROW2 {
    // SAFETY: the row is initialised by the API, then filled.
    let mut row: MIB_IPFORWARD_ROW2 = unsafe { std::mem::zeroed() };
    unsafe { InitializeIpForwardEntry(&mut row) };
    row.InterfaceLuid = luid;
    row.DestinationPrefix.Prefix = ip_sockaddr(net.addr);
    row.DestinationPrefix.PrefixLength = net.prefix;
    row.NextHop = next_hop.unwrap_or_else(|| ip_sockaddr(unspecified(net.addr)));
    row.Metric = 0;
    row
}

fn unspecified(like: IpAddr) -> IpAddr {
    match like {
        IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    }
}

/// Host routes added on the physical interface, removed when the tunnel goes.
#[derive(Default)]
pub struct Routes(Vec<MIB_IPFORWARD_ROW2>);

// The rows are plain data.
unsafe impl Send for Routes {}

impl Routes {
    /// Keeps each server address on the route it uses now, before the
    /// tunnel's half-default routes take over.
    pub fn bypass(&mut self, hosts: &[IpNet]) -> Result<(), String> {
        for host in hosts {
            // SAFETY: outputs are written by the API.
            let mut best: MIB_IPFORWARD_ROW2 = unsafe { std::mem::zeroed() };
            let mut source: SOCKADDR_INET = unsafe { std::mem::zeroed() };
            let destination = ip_sockaddr(host.addr);
            let status = unsafe {
                GetBestRoute2(
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    &destination,
                    0,
                    &mut best,
                    &mut source,
                )
            };
            if status != NO_ERROR {
                return Err(format!(
                    "No route to the server {} (error {status})",
                    host.addr
                ));
            }
            let row = route_row(best.InterfaceLuid, host, Some(best.NextHop));
            // SAFETY: fully initialised row.
            let status = unsafe { CreateIpForwardEntry2(&row) };
            // Already there (another tunnel to the same server): fine.
            if status != NO_ERROR
                && status != windows_sys::Win32::Foundation::ERROR_OBJECT_ALREADY_EXISTS
            {
                return Err(format!(
                    "Cannot keep the server {} reachable (error {status})",
                    host.addr
                ));
            }
            self.0.push(row);
        }
        Ok(())
    }

    pub fn remove_all(&mut self) {
        for row in self.0.drain(..) {
            // SAFETY: a row created by `bypass`.
            unsafe { DeleteIpForwardEntry2(&row) };
        }
    }
}

impl Drop for Routes {
    fn drop(&mut self) {
        self.remove_all();
    }
}

/// The tunnel's DNS servers and search domains, on the adapter (Windows 10
/// 2004 or later). A full tunnel also gets the lowest interface metric, so
/// its DNS servers are asked first.
pub fn set_dns(luid: NET_LUID_LH, servers: &[IpAddr], search: &[String]) -> Result<(), String> {
    if servers.is_empty() && search.is_empty() {
        return Ok(());
    }
    // SAFETY: output written by the API.
    let mut guid: GUID = unsafe { std::mem::zeroed() };
    let status = unsafe { ConvertInterfaceLuidToGuid(&luid, &mut guid) };
    if status != NO_ERROR {
        return Err(format!("Cannot find the tunnel adapter (error {status})"));
    }
    let servers = wide(
        &servers
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(","),
    );
    let search = wide(&search.join(","));
    // SAFETY: plain data; the strings live until the call returns.
    let mut settings: DNS_INTERFACE_SETTINGS = unsafe { std::mem::zeroed() };
    settings.Version = DNS_INTERFACE_SETTINGS_VERSION1;
    settings.Flags = u64::from(DNS_SETTING_NAMESERVER | DNS_SETTING_SEARCHLIST);
    settings.NameServer = servers.as_ptr().cast_mut();
    settings.SearchList = search.as_ptr().cast_mut();
    let status = unsafe { SetInterfaceDnsSettings(guid, &settings) };
    if status != NO_ERROR {
        return Err(format!(
            "Cannot set the tunnel's DNS servers (error {status})"
        ));
    }
    Ok(())
}

/// Cumulative counters of an adapter.
pub fn counters(luid: NET_LUID_LH) -> Option<Counters> {
    // SAFETY: the API reads `InterfaceLuid` and fills the rest.
    let mut row: MIB_IF_ROW2 = unsafe { std::mem::zeroed() };
    row.InterfaceLuid = luid;
    (unsafe { GetIfEntry2(&mut row) } == NO_ERROR).then(|| Counters {
        rx_bytes: row.InOctets,
        tx_bytes: row.OutOctets,
        rx_packets: row.InUcastPkts + row.InNUcastPkts,
        tx_packets: row.OutUcastPkts + row.OutNUcastPkts,
    })
}
