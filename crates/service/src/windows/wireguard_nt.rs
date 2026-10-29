// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! WireGuardNT, the WireGuard® kernel driver for Windows, through its DLL
//! (`wireguard.dll`, signed by its authors, shipped next to the service).
//!
//! The DLL is loaded at run time from the service's own folder only, and its
//! C interface (`wireguard.h`) is declared here by hand.
//!
//! NOT YET RUN ON WINDOWS: the structures below follow `wireguard.h` of
//! WireGuardNT 0.10; check their layout against the header shipped with the
//! DLL before the first test (docs/WINDOWS.md, “First run”).

use std::ffi::c_void;
use std::mem::{align_of, size_of};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use windows_sys::Win32::Foundation::{FreeLibrary, GetLastError, HMODULE};
use windows_sys::Win32::NetworkManagement::Ndis::NET_LUID_LH;
use windows_sys::Win32::Networking::WinSock::{
    AF_INET, AF_INET6, IN_ADDR, IN6_ADDR, SOCKADDR_INET,
};
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_APPLICATION_DIR, LoadLibraryExW,
};
use windows_sys::core::GUID;

use tunnelbird_core::plan::WgPlan;

use super::{net, wide};

type AdapterHandle = *mut c_void;

const STATE_DOWN: i32 = 0;
const STATE_UP: i32 = 1;

const INTERFACE_HAS_PRIVATE_KEY: u32 = 1 << 1;
const INTERFACE_HAS_LISTEN_PORT: u32 = 1 << 2;
const INTERFACE_REPLACE_PEERS: u32 = 1 << 3;

const PEER_HAS_PUBLIC_KEY: u32 = 1 << 0;
const PEER_HAS_PRESHARED_KEY: u32 = 1 << 1;
const PEER_HAS_PERSISTENT_KEEPALIVE: u32 = 1 << 2;
const PEER_HAS_ENDPOINT: u32 = 1 << 3;
const PEER_REPLACE_ALLOWED_IPS: u32 = 1 << 5;

#[repr(C, align(8))]
struct Interface {
    flags: u32,
    listen_port: u16,
    private_key: [u8; 32],
    public_key: [u8; 32],
    peers_count: u32,
}

#[repr(C, align(8))]
struct Peer {
    flags: u32,
    reserved: u32,
    public_key: [u8; 32],
    preshared_key: [u8; 32],
    persistent_keepalive: u16,
    endpoint: SOCKADDR_INET,
    tx_bytes: u64,
    rx_bytes: u64,
    last_handshake: u64,
    allowed_ips_count: u32,
}

#[repr(C)]
union Address {
    v4: IN_ADDR,
    v6: IN6_ADDR,
}

#[repr(C, align(8))]
struct AllowedIp {
    address: Address,
    address_family: u16,
    cidr: u8,
}

/// What GetProcAddress returns, before it is given its real signature.
type Proc = unsafe extern "system" fn() -> isize;
type CreateAdapter =
    unsafe extern "system" fn(*const u16, *const u16, *const GUID) -> AdapterHandle;
type CloseAdapter = unsafe extern "system" fn(AdapterHandle);
type GetAdapterLuid = unsafe extern "system" fn(AdapterHandle, *mut NET_LUID_LH);
type SetAdapterState = unsafe extern "system" fn(AdapterHandle, i32) -> i32;
type SetConfiguration = unsafe extern "system" fn(AdapterHandle, *const Interface, u32) -> i32;

/// The loaded DLL and the functions Tunnelbird uses.
pub struct Driver {
    module: HMODULE,
    create: CreateAdapter,
    close: CloseAdapter,
    luid: GetAdapterLuid,
    set_state: SetAdapterState,
    set_configuration: SetConfiguration,
}

// The DLL's functions may be called from any thread.
unsafe impl Send for Driver {}
unsafe impl Sync for Driver {}

impl Driver {
    pub fn load() -> Result<Self, String> {
        let name = wide("wireguard.dll");
        // SAFETY: plain Win32 calls; the search is limited to the service's
        // folder, so no other wireguard.dll can be picked up.
        unsafe {
            let module = LoadLibraryExW(
                name.as_ptr(),
                std::ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_APPLICATION_DIR,
            );
            if module.is_null() {
                return Err(format!(
                    "The WireGuard driver library is missing (wireguard.dll, error {})",
                    GetLastError()
                ));
            }
            let symbol = |name: &[u8]| {
                GetProcAddress(module, name.as_ptr()).ok_or_else(|| {
                    format!(
                        "wireguard.dll has no {}",
                        String::from_utf8_lossy(&name[..name.len() - 1])
                    )
                })
            };
            Ok(Self {
                create: std::mem::transmute::<Proc, CreateAdapter>(symbol(
                    b"WireGuardCreateAdapter\0",
                )?),
                close: std::mem::transmute::<Proc, CloseAdapter>(symbol(
                    b"WireGuardCloseAdapter\0",
                )?),
                luid: std::mem::transmute::<Proc, GetAdapterLuid>(symbol(
                    b"WireGuardGetAdapterLUID\0",
                )?),
                set_state: std::mem::transmute::<Proc, SetAdapterState>(symbol(
                    b"WireGuardSetAdapterState\0",
                )?),
                set_configuration: std::mem::transmute::<Proc, SetConfiguration>(symbol(
                    b"WireGuardSetConfiguration\0",
                )?),
                module,
            })
        }
    }

    /// Creates the adapter, configures it and brings it up. The returned
    /// [`Adapter`] removes everything when dropped.
    pub fn start(
        self: &Arc<Self>,
        name: &str,
        plan: &WgPlan,
        resolved: &[Option<SocketAddr>],
    ) -> Result<Adapter, String> {
        let name_w = wide(name);
        let tunnel_type = wide("Tunnelbird");
        // SAFETY: pointers to live, NUL-terminated strings; a null GUID lets
        // the driver choose one.
        let handle =
            unsafe { (self.create)(name_w.as_ptr(), tunnel_type.as_ptr(), std::ptr::null()) };
        if handle.is_null() {
            return Err(format!(
                "Cannot create the tunnel adapter (error {})",
                unsafe { GetLastError() }
            ));
        }
        let adapter = Adapter {
            driver: self.clone(),
            handle,
            routes: net::Routes::default(),
        };
        let buffer = configuration(plan, resolved);
        let bytes = u32::try_from(buffer.len() * size_of::<u64>())
            .map_err(|_| "Configuration too large".to_owned())?;
        // SAFETY: `buffer` holds a WIREGUARD_INTERFACE followed by its peers
        // and allowed IPs, 8-byte aligned, `bytes` long.
        if unsafe { (self.set_configuration)(handle, buffer.as_ptr().cast(), bytes) } == 0 {
            return Err(format!(
                "The driver refused the configuration (error {})",
                unsafe { GetLastError() }
            ));
        }
        // SAFETY: valid adapter handle.
        if unsafe { (self.set_state)(handle, STATE_UP) } == 0 {
            return Err(format!("Cannot bring the tunnel up (error {})", unsafe {
                GetLastError()
            }));
        }
        Ok(adapter)
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        // SAFETY: the module was loaded by `load` and no adapter outlives `self`.
        unsafe { FreeLibrary(self.module) };
    }
}

/// A running WireGuardNT adapter.
pub struct Adapter {
    driver: Arc<Driver>,
    handle: AdapterHandle,
    pub routes: net::Routes,
}

// The adapter handle may be used from any thread.
unsafe impl Send for Adapter {}

impl Adapter {
    pub fn luid(&self) -> NET_LUID_LH {
        // SAFETY: an all-zero NET_LUID is a valid value to overwrite.
        let mut luid: NET_LUID_LH = unsafe { std::mem::zeroed() };
        // SAFETY: valid adapter handle, output pointer to `luid`.
        unsafe { (self.driver.luid)(self.handle, &mut luid) };
        luid
    }
}

impl Drop for Adapter {
    fn drop(&mut self) {
        self.routes.remove_all();
        // SAFETY: valid handle, closed once; closing deletes the adapter with
        // its addresses and routes.
        unsafe {
            (self.driver.set_state)(self.handle, STATE_DOWN);
            (self.driver.close)(self.handle);
        }
    }
}

/// Lays out WIREGUARD_INTERFACE, then each WIREGUARD_PEER followed by its
/// WIREGUARD_ALLOWED_IPs, in an 8-byte aligned buffer.
fn configuration(plan: &WgPlan, resolved: &[Option<SocketAddr>]) -> Vec<u64> {
    let mut words = Vec::new();
    let mut push = |bytes: &[u8]| {
        debug_assert!(bytes.len().is_multiple_of(8));
        for chunk in bytes.chunks(8) {
            words.push(u64::from_ne_bytes(chunk.try_into().unwrap_or([0; 8])));
        }
    };
    let mut interface = Interface {
        flags: INTERFACE_HAS_PRIVATE_KEY | INTERFACE_REPLACE_PEERS,
        listen_port: 0,
        private_key: plan.private_key,
        public_key: [0; 32],
        peers_count: plan.peers.len() as u32,
    };
    if let Some(port) = plan.listen_port {
        interface.flags |= INTERFACE_HAS_LISTEN_PORT;
        interface.listen_port = port;
    }
    push(as_bytes(&interface));
    for (index, peer) in plan.peers.iter().enumerate() {
        // SAFETY: SOCKADDR_INET is plain data; zero is “no address”.
        let mut record = Peer {
            flags: PEER_HAS_PUBLIC_KEY | PEER_REPLACE_ALLOWED_IPS,
            reserved: 0,
            public_key: peer.public_key,
            preshared_key: [0; 32],
            persistent_keepalive: 0,
            endpoint: unsafe { std::mem::zeroed() },
            tx_bytes: 0,
            rx_bytes: 0,
            last_handshake: 0,
            allowed_ips_count: peer.allowed_ips.len() as u32,
        };
        if let Some(psk) = peer.preshared_key {
            record.flags |= PEER_HAS_PRESHARED_KEY;
            record.preshared_key = psk;
        }
        if let Some(seconds) = peer.keepalive {
            record.flags |= PEER_HAS_PERSISTENT_KEEPALIVE;
            record.persistent_keepalive = seconds;
        }
        if let Some(Some(addr)) = resolved.get(index) {
            record.flags |= PEER_HAS_ENDPOINT;
            record.endpoint = net::sockaddr(*addr);
        }
        push(as_bytes(&record));
        for net in &peer.allowed_ips {
            let ip = match net.addr {
                IpAddr::V4(v4) => AllowedIp {
                    address: Address {
                        v4: net::in_addr(v4),
                    },
                    address_family: AF_INET,
                    cidr: net.prefix,
                },
                IpAddr::V6(v6) => AllowedIp {
                    address: Address {
                        v6: net::in6_addr(v6),
                    },
                    address_family: AF_INET6,
                    cidr: net.prefix,
                },
            };
            push(as_bytes(&ip));
        }
    }
    words
}

/// The bytes of a `repr(C)` value, padding included.
fn as_bytes<T>(value: &T) -> &[u8] {
    const { assert!(align_of::<T>() == 8 && size_of::<T>().is_multiple_of(8)) };
    // SAFETY: reading the bytes of a plain `repr(C)` value.
    unsafe { std::slice::from_raw_parts((value as *const T).cast::<u8>(), size_of::<T>()) }
}
