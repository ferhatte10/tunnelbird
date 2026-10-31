// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! The logs of each tunnel on the systems with a tunnel service: what the
//! service saw happen and what the VPN program wrote, kept in memory for the
//! session, as `src/logs.rs` does in the Linux app.

use std::collections::{HashMap, VecDeque};

use serde::{Deserialize, Serialize};

/// Most lines kept per tunnel: the oldest ones go first.
const MAX_LINES: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// Tunnelbird's own service.
    Service,
    Wireguard,
    Openvpn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogLine {
    /// Seconds since 1970.
    pub time: i64,
    pub source: Source,
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct LogBook {
    tunnels: HashMap<String, VecDeque<LogLine>>,
}

impl LogBook {
    pub fn push(&mut self, tunnel: &str, line: LogLine) {
        let lines = self.tunnels.entry(tunnel.to_owned()).or_default();
        lines.push_back(line);
        while lines.len() > MAX_LINES {
            lines.pop_front();
        }
    }

    pub fn lines(&self, tunnel: &str) -> Vec<LogLine> {
        self.tunnels
            .get(tunnel)
            .map(|lines| lines.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn forget(&mut self, tunnel: &str) {
        self.tunnels.remove(tunnel);
    }
}

/// How serious a line of OpenVPN's log is, and its text without the date it
/// starts with (the line gets its own time).
pub fn read_openvpn_line(line: &str) -> (Severity, &str) {
    let text = strip_date(line.trim());
    let upper = text.to_uppercase();
    let severity = if ["AUTH_FAILED", "ERROR", "FATAL", "EXITING DUE TO", "CANNOT "]
        .iter()
        .any(|word| upper.contains(word))
    {
        Severity::Error
    } else if [
        "WARNING",
        "DEPRECATED",
        "TIMEOUT",
        "TIMED OUT",
        "SIGUSR1",
        "SIGTERM",
    ]
    .iter()
    .any(|word| upper.contains(word))
    {
        Severity::Warning
    } else {
        Severity::Info
    };
    (severity, text)
}

/// Removes a leading `2026-10-31 01:30:00 `.
fn strip_date(line: &str) -> &str {
    let bytes = line.as_bytes();
    let dated = bytes.len() > 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b' '
        && bytes[13] == b':'
        && bytes[19] == b' ';
    if dated {
        line.get(20..).unwrap_or(line)
    } else {
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(message: &str) -> LogLine {
        LogLine {
            time: 0,
            source: Source::Service,
            severity: Severity::Info,
            message: message.to_owned(),
        }
    }

    #[test]
    fn the_book_is_bounded_and_keeps_tunnels_apart() {
        let mut book = LogBook::default();
        for n in 0..MAX_LINES + 5 {
            book.push("a", line(&n.to_string()));
        }
        book.push("b", line("other"));
        let lines = book.lines("a");
        assert_eq!(lines.len(), MAX_LINES);
        assert_eq!(lines[0].message, "5");
        assert_eq!(book.lines("b").len(), 1);
        book.forget("a");
        assert!(book.lines("a").is_empty());
    }

    #[test]
    fn openvpn_lines_get_a_severity_and_lose_their_date() {
        assert_eq!(
            read_openvpn_line("2026-10-31 01:30:00 Initialization Sequence Completed\n"),
            (Severity::Info, "Initialization Sequence Completed")
        );
        assert_eq!(
            read_openvpn_line("2026-10-31 01:30:00 AUTH: Received control message: AUTH_FAILED").0,
            Severity::Error
        );
        assert_eq!(
            read_openvpn_line("2026-10-31 01:30:00 WARNING: this cipher is weak").0,
            Severity::Warning
        );
        assert_eq!(read_openvpn_line("short"), (Severity::Info, "short"));
    }
}
