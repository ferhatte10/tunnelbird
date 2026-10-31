// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! What the service does with each request, whatever the system: validate
//! files with the core parsers, keep the tunnels in a [`Store`], connect one
//! tunnel at a time through a [`Backend`], and say what changed as events.
//! No I/O here besides the store and the backend, so all of it is tested
//! with fakes on any system.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use tunnelbird_core::config;
use tunnelbird_core::health::{Health, HealthWatch};
use tunnelbird_core::logs::{LogBook, LogLine, Severity, Source, read_openvpn_line};
use tunnelbird_core::profile::{self, Profile};
use tunnelbird_core::protocol::{
    Counters, ErrorCode, Event, Login, RequestBody, ResponseBody, State, Tunnel, VERSION,
};
use tunnelbird_core::provider::{self, Hints};

/// A tunnel as stored: the file the user approved, and the OpenVPN login.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stored {
    pub id: String,
    pub name: String,
    pub text: String,
    #[serde(default)]
    pub login: Option<Login>,
}

/// Where tunnels are kept between runs. Implementations must keep them
/// readable by the service only (keys and passwords are inside).
pub trait Store {
    fn load(&self) -> Result<Vec<Stored>, String>;
    fn save(&mut self, tunnel: &Stored) -> Result<(), String>;
    fn delete(&mut self, id: &str) -> Result<(), String>;
}

/// The system's VPN drivers (WireGuardNT and ovpn-dco on Windows, a userspace
/// tunnel on macOS).
pub trait Backend {
    /// Brings the tunnel up; returns once it is up, or why it failed.
    fn start(&mut self, id: &str, profile: &Profile, login: Option<&Login>)
    -> Result<Link, String>;
    fn stop(&mut self, id: &str) -> Result<(), String>;
    /// Cumulative counters of a running tunnel.
    fn counters(&self, id: &str) -> Option<Counters>;
    /// What the tunnel reported since the last call.
    fn take_logs(&mut self, _id: &str) -> Vec<BackendLine> {
        Vec::new()
    }
    /// Why the running tunnel stopped on its own, if it did.
    fn stopped(&mut self, _id: &str) -> Option<String> {
        None
    }
}

/// A line for the tunnel's logs. Only the macOS backend writes any so far.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum BackendLine {
    /// As OpenVPN wrote it: its severity is read from the text.
    Openvpn(String),
    Wireguard(Severity, String),
}

/// What a started tunnel reports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Link {
    /// This computer's addresses inside the VPN.
    pub addresses: Vec<String>,
    /// All traffic goes through the tunnel, when the running tunnel knows
    /// better than its file: an OpenVPN server decides the routes.
    pub full_tunnel: Option<bool>,
}

pub struct Service<B, S> {
    backend: B,
    store: S,
    tunnels: Vec<Stored>,
    /// The connected tunnel and its addresses; one at a time.
    connected: Option<(String, Link)>,
    health: HealthWatch,
    last_health: Health,
    logs: LogBook,
}

/// The answer to a request and what other clients must hear about.
pub type Outcome = (ResponseBody, Vec<Event>);

impl<B: Backend, S: Store> Service<B, S> {
    pub fn new(backend: B, store: S) -> Result<Self, String> {
        let tunnels = store.load()?;
        Ok(Self {
            backend,
            store,
            tunnels,
            connected: None,
            health: HealthWatch::default(),
            last_health: Health::default(),
            logs: LogBook::default(),
        })
    }

    pub fn handle(&mut self, request: RequestBody) -> Outcome {
        match request {
            RequestBody::Hello { version } if version == VERSION => (
                ResponseBody::Hello {
                    version: VERSION,
                    service_version: Some(env!("CARGO_PKG_VERSION").to_owned()),
                },
                Vec::new(),
            ),
            RequestBody::Hello { version } => error(
                ErrorCode::VersionMismatch,
                format!("The service speaks protocol {VERSION}, the interface {version}"),
            ),
            RequestBody::ListTunnels => (
                ResponseBody::Tunnels {
                    tunnels: self.list(),
                },
                Vec::new(),
            ),
            RequestBody::Import {
                name,
                text,
                login,
                files,
            } => self.import(name, text, login, files),
            RequestBody::Update {
                tunnel,
                name,
                text,
                login,
            } => self.update(&tunnel, name, text, login),
            RequestBody::Delete { tunnel } => self.delete(&tunnel),
            RequestBody::Connect { tunnel } => self.connect(&tunnel),
            RequestBody::Disconnect { tunnel } => self.disconnect(&tunnel),
            RequestBody::TrustDecision { policy, network } => (
                ResponseBody::TrustDecision {
                    decision: policy.decide(&network),
                },
                Vec::new(),
            ),
            RequestBody::Logs { tunnel } => match self.find(&tunnel) {
                Some(_) => (
                    ResponseBody::Logs {
                        lines: self.logs.lines(&tunnel),
                    },
                    Vec::new(),
                ),
                None => unknown(&tunnel),
            },
            RequestBody::Export { tunnel } => match self.find(&tunnel) {
                Some(stored) => (
                    ResponseBody::Text {
                        text: stored.text.clone(),
                    },
                    Vec::new(),
                ),
                None => unknown(&tunnel),
            },
        }
    }

    /// Called once a second: traffic of the connected tunnel.
    pub fn tick(&mut self) -> Vec<Event> {
        let Some(id) = self.connected.as_ref().map(|(id, _)| id.clone()) else {
            return Vec::new();
        };
        let mut events = self.backend_logs(&id);
        if let Some(reason) = self.backend.stopped(&id) {
            let message = format!("Stopped on its own: {reason}");
            events.push(self.log(&id, Severity::Error, message));
            events.extend(self.stop_connected(Some(reason)));
            events.push(self.changed());
            return events;
        }
        let Some(counters) = self.backend.counters(&id) else {
            return events;
        };
        let health = self
            .health
            .record(&id, counters.rx_packets, counters.tx_packets);
        if health != self.last_health {
            if health == Health::NoResponse {
                events.push(self.log(&id, Severity::Warning, "Server not responding".to_owned()));
            } else if self.last_health == Health::NoResponse {
                events.push(self.log(&id, Severity::Info, "Server responding again".to_owned()));
            }
            self.last_health = health;
        }
        events.push(Event::Traffic {
            tunnel: id,
            counters,
            health: Some(health),
        });
        events
    }

    /// Records a line from the service itself.
    fn log(&mut self, id: &str, severity: Severity, message: String) -> Event {
        self.record(id, Source::Service, severity, message)
    }

    fn record(&mut self, id: &str, source: Source, severity: Severity, message: String) -> Event {
        let line = LogLine {
            time: unix_now(),
            source,
            severity,
            message,
        };
        self.logs.push(id, line.clone());
        Event::Log {
            tunnel: id.to_owned(),
            line,
        }
    }

    fn backend_logs(&mut self, id: &str) -> Vec<Event> {
        let lines = self.backend.take_logs(id);
        lines
            .into_iter()
            .map(|line| match line {
                BackendLine::Openvpn(line) => {
                    let (severity, text) = read_openvpn_line(&line);
                    self.record(id, Source::Openvpn, severity, text.to_owned())
                }
                BackendLine::Wireguard(severity, text) => {
                    self.record(id, Source::Wireguard, severity, text)
                }
            })
            .collect()
    }

    pub fn list(&self) -> Vec<Tunnel> {
        self.tunnels.iter().map(|t| self.summary(t)).collect()
    }

    fn find(&self, id: &str) -> Option<&Stored> {
        self.tunnels.iter().find(|t| t.id == id)
    }

    fn import(
        &mut self,
        name: String,
        text: String,
        login: Option<Login>,
        files: BTreeMap<String, String>,
    ) -> Outcome {
        let text = match with_files_inside(text, &files) {
            Ok(text) => text,
            Err(message) => return error(ErrorCode::InvalidProfile, message),
        };
        if let Err(message) = validate(&text, login.as_ref()) {
            return error(ErrorCode::InvalidProfile, message);
        }
        let stored = Stored {
            id: uuid::Uuid::new_v4().to_string(),
            name: unique_name(&self.tunnels, name.trim(), None),
            text,
            login,
        };
        if let Err(message) = self.store.save(&stored) {
            return error(ErrorCode::Internal, message);
        }
        let tunnel = self.summary(&stored);
        self.tunnels.push(stored);
        (ResponseBody::Tunnel { tunnel }, vec![self.changed()])
    }

    fn update(&mut self, id: &str, name: String, text: String, login: Option<Login>) -> Outcome {
        let Some(existing) = self.find(id) else {
            return unknown(id);
        };
        // The login never leaves the service: editing a file keeps it.
        let login = login.or_else(|| existing.login.clone());
        if let Err(message) = validate(&text, login.as_ref()) {
            return error(ErrorCode::InvalidProfile, message);
        }
        let stored = Stored {
            id: id.to_owned(),
            name: unique_name(&self.tunnels, name.trim(), Some(id)),
            text,
            login,
        };
        if let Err(message) = self.store.save(&stored) {
            return error(ErrorCode::Internal, message);
        }
        let tunnel = self.summary(&stored);
        if let Some(slot) = self.tunnels.iter_mut().find(|t| t.id == id) {
            // A connected tunnel keeps running with its old settings until
            // the interface reconnects it.
            *slot = stored;
        }
        (ResponseBody::Tunnel { tunnel }, vec![self.changed()])
    }

    fn delete(&mut self, id: &str) -> Outcome {
        if self.find(id).is_none() {
            return unknown(id);
        }
        let mut events = Vec::new();
        if self.is_connected(id) {
            events.extend(self.stop_connected(None));
        }
        if let Err(message) = self.store.delete(id) {
            return error(ErrorCode::Internal, message);
        }
        self.tunnels.retain(|t| t.id != id);
        self.logs.forget(id);
        events.push(self.changed());
        (ResponseBody::Ok, events)
    }

    fn connect(&mut self, id: &str) -> Outcome {
        let Some(stored) = self.find(id).cloned() else {
            return unknown(id);
        };
        if self.is_connected(id) {
            return (ResponseBody::Ok, Vec::new());
        }
        let profile = match validate(&stored.text, stored.login.as_ref()) {
            Ok(profile) => profile,
            Err(message) => return error(ErrorCode::InvalidProfile, message),
        };
        // One at a time: the current tunnel goes down first.
        let mut events = self.stop_connected(None);
        if needs_password(&profile, stored.login.as_ref()) {
            if !events.is_empty() {
                events.push(self.changed());
            }
            events.push(Event::PasswordNeeded {
                tunnel: id.to_owned(),
            });
            return (ResponseBody::Ok, events);
        }
        events.push(self.log(id, Severity::Info, "Connecting…".to_owned()));
        events.push(state(id, State::Connecting, None));
        match self.backend.start(id, &profile, stored.login.as_ref()) {
            Ok(link) => {
                self.connected = Some((id.to_owned(), link));
                events.extend(self.backend_logs(id));
                events.push(self.log(id, Severity::Info, "Connected".to_owned()));
                events.push(state(id, State::Connected, None));
                events.push(self.changed());
                (ResponseBody::Ok, events)
            }
            Err(message) => {
                events.push(self.log(id, Severity::Error, format!("Cannot connect: {message}")));
                events.push(state(id, State::Disconnected, Some(message.clone())));
                events.push(self.changed());
                (
                    ResponseBody::Error {
                        code: ErrorCode::ConnectionFailed,
                        message,
                    },
                    events,
                )
            }
        }
    }

    fn disconnect(&mut self, id: &str) -> Outcome {
        if self.find(id).is_none() {
            return unknown(id);
        }
        if !self.is_connected(id) {
            return (ResponseBody::Ok, Vec::new());
        }
        let mut events = self.stop_connected(None);
        events.push(self.changed());
        (ResponseBody::Ok, events)
    }

    fn is_connected(&self, id: &str) -> bool {
        self.connected
            .as_ref()
            .is_some_and(|(current, _)| current == id)
    }

    /// Takes the connected tunnel down, if any, with its state events (the
    /// caller adds the new list once its own change is done).
    fn stop_connected(&mut self, error: Option<String>) -> Vec<Event> {
        let Some((id, _)) = self.connected.take() else {
            return Vec::new();
        };
        self.health.reset();
        self.last_health = Health::default();
        let mut events = vec![state(&id, State::Disconnecting, None)];
        let error = match self.backend.stop(&id) {
            Ok(()) => error,
            Err(message) => Some(message),
        };
        events.extend(self.backend_logs(&id));
        events.push(self.log(&id, Severity::Info, "Disconnected".to_owned()));
        events.push(state(&id, State::Disconnected, error));
        events
    }

    fn changed(&self) -> Event {
        Event::TunnelsChanged {
            tunnels: self.list(),
        }
    }

    fn summary(&self, stored: &Stored) -> Tunnel {
        let profile = profile::parse_bytes(stored.text.as_bytes()).ok();
        let (endpoint, full_tunnel, dns) = match &profile {
            Some(Profile::WireGuard(config)) => (
                config.peers.first().and_then(|p| p.endpoint.clone()),
                config.is_full_tunnel(),
                config
                    .interface
                    .dns_servers
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            ),
            Some(Profile::OpenVpn(config)) => {
                (config.endpoint(), config.redirect_gateway, Vec::new())
            }
            None => (None, false, Vec::new()),
        };
        let provider = provider::detect(&Hints {
            name: &stored.name,
            endpoint: endpoint.as_deref(),
            dns: &dns,
        });
        let (state, addresses, full_tunnel) = match &self.connected {
            Some((id, link)) if *id == stored.id => (
                State::Connected,
                link.addresses.clone(),
                link.full_tunnel.unwrap_or(full_tunnel),
            ),
            _ => (
                State::Disconnected,
                wireguard_addresses(profile.as_ref()),
                full_tunnel,
            ),
        };
        Tunnel {
            id: stored.id.clone(),
            name: stored.name.clone(),
            kind: profile.as_ref().map(Profile::kind).unwrap_or_default(),
            endpoint,
            addresses,
            full_tunnel,
            provider: provider.map(str::to_owned),
            state,
        }
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

/// Checks a file as the Linux app does at import. OpenVPN certificates must
/// be inline: the service never reads the user's files.
fn with_files_inside(text: String, files: &BTreeMap<String, String>) -> Result<String, String> {
    if !files.is_empty()
        && let Ok(Profile::OpenVpn(mut config)) = profile::parse_bytes(text.as_bytes())
    {
        config
            .resolve_files(|name| {
                let base = std::path::Path::new(name).file_name()?.to_str()?;
                files.get(base).cloned()
            })
            .map_err(|err| err.to_string())?;
        return Ok(config.to_ovpn());
    }
    Ok(text)
}

pub fn validate(text: &str, login: Option<&Login>) -> Result<Profile, String> {
    let mut profile = profile::parse_bytes(text.as_bytes())?;
    if let Profile::OpenVpn(config) = &mut profile {
        config
            .resolve_files(|_| None)
            .and_then(|()| config.check())
            .map_err(|err| err.to_string())?;
        if config.user_pass && login.is_none_or(|l| l.username.trim().is_empty()) {
            return Err("This profile needs a user name".to_owned());
        }
    }
    Ok(profile)
}

fn needs_password(profile: &Profile, login: Option<&Login>) -> bool {
    matches!(profile, Profile::OpenVpn(config) if config.user_pass)
        && login.is_none_or(|l| l.password.is_empty())
}

/// WireGuard® addresses are in the file; OpenVPN's come from the server.
fn wireguard_addresses(profile: Option<&Profile>) -> Vec<String> {
    match profile {
        Some(Profile::WireGuard(config)) => config::join_nets(&config.interface.addresses)
            .split(", ")
            .filter(|a| !a.is_empty())
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

/// `name`, or `name (2)`… if another tunnel (not `except`) has it.
fn unique_name(tunnels: &[Stored], name: &str, except: Option<&str>) -> String {
    let taken = |candidate: &str| {
        tunnels
            .iter()
            .any(|t| t.name == candidate && Some(t.id.as_str()) != except)
    };
    let mut unique = name.to_owned();
    let mut n = 2;
    while taken(&unique) {
        unique = format!("{name} ({n})");
        n += 1;
    }
    unique
}

fn state(id: &str, state: State, error: Option<String>) -> Event {
    Event::State {
        tunnel: id.to_owned(),
        state,
        error,
    }
}

fn error(code: ErrorCode, message: String) -> Outcome {
    (ResponseBody::Error { code, message }, Vec::new())
}

fn unknown(id: &str) -> Outcome {
    error(ErrorCode::UnknownTunnel, format!("No tunnel {id}"))
}

/// In-memory store and scripted backend, for tests and `--fake` runs.
pub mod fake {
    use super::*;

    #[derive(Default)]
    pub struct MemoryStore(pub HashMap<String, Stored>);

    impl Store for MemoryStore {
        fn load(&self) -> Result<Vec<Stored>, String> {
            let mut list: Vec<Stored> = self.0.values().cloned().collect();
            list.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(list)
        }
        fn save(&mut self, tunnel: &Stored) -> Result<(), String> {
            self.0.insert(tunnel.id.clone(), tunnel.clone());
            Ok(())
        }
        fn delete(&mut self, id: &str) -> Result<(), String> {
            self.0.remove(id);
            Ok(())
        }
    }

    /// Connects anything except servers named `unreachable…`; counters grow
    /// by one megabyte per reading.
    #[derive(Default)]
    pub struct FakeBackend {
        pub running: HashMap<String, std::cell::Cell<u64>>,
    }

    impl Backend for FakeBackend {
        fn start(
            &mut self,
            id: &str,
            profile: &Profile,
            _: Option<&Login>,
        ) -> Result<Link, String> {
            let endpoint = match profile {
                Profile::WireGuard(c) => c.peers.first().and_then(|p| p.endpoint.clone()),
                Profile::OpenVpn(c) => c.endpoint(),
            };
            if endpoint.is_some_and(|e| e.starts_with("unreachable")) {
                return Err("The server does not answer".to_owned());
            }
            self.running.insert(id.to_owned(), std::cell::Cell::new(0));
            Ok(Link {
                addresses: vec!["10.99.0.2/24".to_owned()],
                ..Link::default()
            })
        }
        fn stop(&mut self, id: &str) -> Result<(), String> {
            self.running.remove(id);
            Ok(())
        }
        fn counters(&self, id: &str) -> Option<Counters> {
            let readings = self.running.get(id)?;
            readings.set(readings.get() + 1);
            let n = readings.get();
            Some(Counters {
                rx_bytes: n * 1_000_000,
                tx_bytes: n * 100_000,
                rx_packets: n * 700,
                tx_packets: n * 300,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::{FakeBackend, MemoryStore};
    use super::*;

    const WG: &str = "[Interface]\nPrivateKey = AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=\nAddress = 10.8.0.2/32\n[Peer]\nPublicKey = AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=\nEndpoint = vpn.example.com:51820\nAllowedIPs = 0.0.0.0/0\n";
    const OVPN: &str = "client\nremote vpn.example.com 1194\nauth-user-pass\n<ca>\nX\n</ca>\n";

    fn service() -> Service<FakeBackend, MemoryStore> {
        Service::new(FakeBackend::default(), MemoryStore::default()).expect("empty store")
    }

    fn import(
        service: &mut Service<FakeBackend, MemoryStore>,
        name: &str,
        text: &str,
        login: Option<Login>,
    ) -> String {
        match service.handle(RequestBody::Import {
            name: name.to_owned(),
            text: text.to_owned(),
            login,
            files: BTreeMap::new(),
        }) {
            (ResponseBody::Tunnel { tunnel }, _) => tunnel.id,
            other => panic!("import failed: {other:?}"),
        }
    }

    #[test]
    fn version_must_match() {
        let mut service = service();
        assert!(matches!(
            service.handle(RequestBody::Hello { version: VERSION }).0,
            ResponseBody::Hello { .. }
        ));
        assert!(matches!(
            service
                .handle(RequestBody::Hello {
                    version: VERSION + 1
                })
                .0,
            ResponseBody::Error {
                code: ErrorCode::VersionMismatch,
                ..
            }
        ));
    }

    #[test]
    fn import_validates_and_names_uniquely() {
        let mut service = service();
        import(&mut service, "Home", WG, None);
        let second = import(&mut service, "Home", WG, None);
        let tunnels = service.list();
        assert_eq!(tunnels.len(), 2);
        let renamed = tunnels.iter().find(|t| t.id == second).expect("listed");
        assert_eq!(renamed.name, "Home (2)");
        assert!(renamed.full_tunnel);
        assert_eq!(renamed.addresses, vec!["10.8.0.2/32"]);
        let (response, _) = service.handle(RequestBody::Import {
            name: "Bad".to_owned(),
            text: "client\nremote a.example\nca ca.crt\nauth-user-pass\n".to_owned(),
            login: None,
            files: BTreeMap::new(),
        });
        assert!(matches!(
            response,
            ResponseBody::Error { code: ErrorCode::InvalidProfile, message } if message.contains("ca.crt")
        ));
    }

    #[test]
    fn one_tunnel_at_a_time() {
        let mut service = service();
        let first = import(&mut service, "A", WG, None);
        let second = import(&mut service, "B", WG, None);
        service.handle(RequestBody::Connect {
            tunnel: first.clone(),
        });
        let (response, events) = service.handle(RequestBody::Connect {
            tunnel: second.clone(),
        });
        assert_eq!(response, ResponseBody::Ok);
        assert!(events.contains(&state(&first, State::Disconnected, None)));
        assert!(events.contains(&state(&second, State::Connected, None)));
        assert!(
            matches!(service.tick().as_slice(), [Event::Traffic { tunnel, .. }] if *tunnel == second)
        );
        let states: Vec<State> = service.list().iter().map(|t| t.state).collect();
        assert_eq!(states, vec![State::Disconnected, State::Connected]);
    }

    #[test]
    fn logs_tell_what_happened_and_why() {
        let mut service = service();
        let good = import(&mut service, "A", WG, None);
        let bad = import(
            &mut service,
            "B",
            &WG.replace("vpn.example.com", "unreachable"),
            None,
        );

        service.handle(RequestBody::Connect {
            tunnel: bad.clone(),
        });
        let (response, _) = service.handle(RequestBody::Logs {
            tunnel: bad.clone(),
        });
        let ResponseBody::Logs { lines } = response else {
            panic!("no logs");
        };
        assert_eq!(lines.last().map(|l| l.severity), Some(Severity::Error));
        assert!(lines.iter().any(|l| l.message.contains("does not answer")));

        let (_, events) = service.handle(RequestBody::Connect {
            tunnel: good.clone(),
        });
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::Log { tunnel, line }
            if *tunnel == good && line.message == "Connected"))
        );
        service.handle(RequestBody::Disconnect {
            tunnel: good.clone(),
        });
        let (response, _) = service.handle(RequestBody::Logs { tunnel: good });
        let ResponseBody::Logs { lines } = response else {
            panic!("no logs");
        };
        let messages: Vec<&str> = lines.iter().map(|l| l.message.as_str()).collect();
        assert_eq!(messages, ["Connecting…", "Connected", "Disconnected"]);
        assert_eq!(
            service
                .handle(RequestBody::Logs {
                    tunnel: "nope".to_owned()
                })
                .0,
            unknown("nope").0
        );
    }

    #[test]
    fn editing_keeps_the_login_unless_a_new_one_comes() {
        let mut service = service();
        let login = Login {
            username: "alex".to_owned(),
            password: "secret".to_owned(),
        };
        let tunnel = import(&mut service, "A", OVPN, Some(login.clone()));
        let update = |text: &str, login| RequestBody::Update {
            tunnel: tunnel.clone(),
            name: "B".to_owned(),
            text: text.to_owned(),
            login,
        };
        let (response, _) = service.handle(update(OVPN, None));
        assert!(matches!(response, ResponseBody::Tunnel { .. }));
        assert_eq!(
            service.find(&tunnel).and_then(|t| t.login.clone()),
            Some(login)
        );
        let new = Login {
            username: "sam".to_owned(),
            password: String::new(),
        };
        service.handle(update(OVPN, Some(new.clone())));
        assert_eq!(
            service.find(&tunnel).and_then(|t| t.login.clone()),
            Some(new)
        );
    }

    #[test]
    fn the_trust_rule_answers_without_connecting_anything() {
        use tunnelbird_core::trust::{Decision, Network, NetworkKind, Policy};
        let mut service = service();
        let tunnel = import(&mut service, "A", WG, None);
        let policy = Policy {
            enabled: true,
            tunnel,
            trusted_ssids: vec!["Home".to_owned()],
            ..Policy::default()
        };
        let joined = |ssid: &str| Network {
            uuid: ssid.to_owned(),
            kind: NetworkKind::Wifi,
            ssid: Some(ssid.to_owned()),
            bssid: None,
        };
        let ask = |service: &mut Service<_, _>, ssid| {
            service.handle(RequestBody::TrustDecision {
                policy: policy.clone(),
                network: joined(ssid),
            })
        };
        assert_eq!(
            ask(&mut service, "Café").0,
            ResponseBody::TrustDecision {
                decision: Decision::Connect
            }
        );
        assert_eq!(
            ask(&mut service, "Home").0,
            ResponseBody::TrustDecision {
                decision: Decision::Stay
            }
        );
        assert!(service.tick().is_empty());
    }

    #[test]
    fn files_next_to_a_profile_are_stored_inside_it() {
        let mut service = service();
        let text = "client\nremote vpn.example.com 1194\nca folder/ca.crt\nauth-user-pass\n";
        let login = Login {
            username: "alex".to_owned(),
            password: "secret".to_owned(),
        };
        let request = |files: BTreeMap<String, String>| RequestBody::Import {
            name: "A".to_owned(),
            text: text.to_owned(),
            login: Some(login.clone()),
            files,
        };
        let (response, _) = service.handle(request(BTreeMap::new()));
        assert!(matches!(
            response,
            ResponseBody::Error {
                code: ErrorCode::InvalidProfile,
                ..
            }
        ));

        let files = BTreeMap::from([("ca.crt".to_owned(), "X\n".to_owned())]);
        let (response, _) = service.handle(request(files));
        let ResponseBody::Tunnel { tunnel } = response else {
            panic!("not imported: {response:?}");
        };
        let exported = service.handle(RequestBody::Export { tunnel: tunnel.id }).0;
        let ResponseBody::Text { text } = exported else {
            panic!("no text");
        };
        assert!(text.contains("<ca>\nX\n</ca>"), "{text}");
        assert!(!text.contains("folder/ca.crt"));
    }

    #[test]
    fn hello_says_which_version_of_the_service_this_is() {
        assert_eq!(
            service().handle(RequestBody::Hello { version: VERSION }).0,
            ResponseBody::Hello {
                version: VERSION,
                service_version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            }
        );
    }

    #[test]
    fn traffic_events_carry_the_server_health() {
        let mut service = service();
        let tunnel = import(&mut service, "A", WG, None);
        service.handle(RequestBody::Connect { tunnel });
        assert!(!service.tick().is_empty());
        // The fake server answers: packets come back from the second reading.
        assert!(service.tick().iter().any(|e| matches!(
            e,
            Event::Traffic {
                health: Some(Health::Ok),
                ..
            }
        )));
    }

    #[test]
    fn openvpn_needs_a_user_name_and_asks_for_the_password() {
        let mut service = service();
        let (response, _) = service.handle(RequestBody::Import {
            name: "Office".to_owned(),
            text: OVPN.to_owned(),
            login: None,
            files: BTreeMap::new(),
        });
        assert!(matches!(
            response,
            ResponseBody::Error {
                code: ErrorCode::InvalidProfile,
                ..
            }
        ));
        let login = Login {
            username: "alex".to_owned(),
            password: String::new(),
        };
        let id = import(&mut service, "Office", OVPN, Some(login));
        let (response, events) = service.handle(RequestBody::Connect { tunnel: id.clone() });
        assert_eq!(response, ResponseBody::Ok);
        assert_eq!(events, vec![Event::PasswordNeeded { tunnel: id }]);
    }

    #[test]
    fn failed_connection_says_why() {
        let mut service = service();
        let id = import(
            &mut service,
            "Down",
            &WG.replace("vpn.example.com", "unreachable.example.com"),
            None,
        );
        let (response, events) = service.handle(RequestBody::Connect { tunnel: id.clone() });
        assert!(matches!(
            response,
            ResponseBody::Error {
                code: ErrorCode::ConnectionFailed,
                ..
            }
        ));
        assert!(events.contains(&state(
            &id,
            State::Disconnected,
            Some("The server does not answer".to_owned())
        )));
        assert!(service.tick().is_empty());
    }

    #[test]
    fn deleting_the_connected_tunnel_stops_it() {
        let mut service = service();
        let id = import(&mut service, "A", WG, None);
        service.handle(RequestBody::Connect { tunnel: id.clone() });
        let (response, events) = service.handle(RequestBody::Delete { tunnel: id.clone() });
        assert_eq!(response, ResponseBody::Ok);
        assert!(events.contains(&state(&id, State::Disconnected, None)));
        assert!(service.list().is_empty());
        assert!(matches!(
            service.handle(RequestBody::Export { tunnel: id }).0,
            ResponseBody::Error {
                code: ErrorCode::UnknownTunnel,
                ..
            }
        ));
    }
}
