# Changes to BoringTun

Copy of the `boringtun` 0.7.1 crate from crates.io (BSD-3-Clause, see
`LICENSE`), used through `[patch.crates-io]` in the root `Cargo.toml`.

One change, for the macOS tunnel service: the UAPI `fwmark` setting is accepted
on macOS and carries an interface index, which binds the listening sockets to
that interface (`IP_BOUND_IF`, `IPV6_BOUND_IF`). See `Device::set_fwmark` in
`src/device/mod.rs` and the `fwmark` arm in `src/device/api.rs`.

Without it a full tunnel needs a host route to the server on the physical
network, and any website hosted on the server's address stays out of the tunnel.

To update: copy the new crate over this folder, then apply the change again.
