// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Detects a "connected" tunnel whose server does not answer.
//!
//! The tunnel interface is up as soon as NetworkManager configures it, even when
//! the server is down, unreachable or rejects the keys: packets leave, nothing
//! comes back, and the system still says "connected". Every second the window
//! records the interface packet counters; if this computer sent packets during
//! the last [`WINDOW`] seconds and received none, the server is not responding.
//!
//! The kernel does not count keepalive packets, so an idle tunnel (nothing
//! sent) stays [`Health::Unknown`] rather than being reported as broken.

use std::collections::VecDeque;

/// Seconds of silence, while sending, before the server is reported as not responding.
/// Long enough for the handshake retries a slow server needs.
const WINDOW: usize = 15;
/// Packets that must have been sent in the window for the silence to mean something.
const MIN_SENT: u64 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Health {
    /// Not enough traffic yet to tell.
    #[default]
    Unknown,
    /// Packets come back from the server.
    Ok,
    /// Packets go out, none come back.
    NoResponse,
}

#[derive(Debug, Default)]
pub struct HealthWatch {
    interface: String,
    /// `(received, sent)` packet counters, one per second, oldest first.
    samples: VecDeque<(u64, u64)>,
}

impl HealthWatch {
    /// Records the counters of `interface` and returns the current verdict.
    /// A different interface (another tunnel) starts a fresh watch.
    pub fn record(&mut self, interface: &str, received: u64, sent: u64) -> Health {
        if self.interface != interface {
            self.interface = interface.to_owned();
            self.samples.clear();
        }
        self.samples.push_back((received, sent));
        while self.samples.len() > WINDOW + 1 {
            self.samples.pop_front();
        }
        let (Some(&(rx_start, tx_start)), Some(&(rx_now, tx_now))) =
            (self.samples.front(), self.samples.back())
        else {
            return Health::Unknown;
        };
        if rx_now > rx_start {
            Health::Ok
        } else if self.samples.len() > WINDOW && tx_now.saturating_sub(tx_start) >= MIN_SENT {
            Health::NoResponse
        } else {
            Health::Unknown
        }
    }

    pub fn reset(&mut self) {
        self.interface.clear();
        self.samples.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(
        watch: &mut HealthWatch,
        seconds: u64,
        rx_per_s: u64,
        tx_per_s: u64,
        start: (u64, u64),
    ) -> Health {
        let mut health = Health::Unknown;
        for s in 1..=seconds {
            health = watch.record("wg0", start.0 + s * rx_per_s, start.1 + s * tx_per_s);
        }
        health
    }

    #[test]
    fn answers_mean_ok() {
        let mut watch = HealthWatch::default();
        assert_eq!(run(&mut watch, 3, 2, 2, (0, 0)), Health::Ok);
    }

    #[test]
    fn sending_without_answer_is_detected_after_the_window_only() {
        let mut watch = HealthWatch::default();
        assert_eq!(
            run(&mut watch, WINDOW as u64 - 1, 0, 1, (0, 0)),
            Health::Unknown
        );
        assert_eq!(run(&mut watch, 5, 0, 1, (0, 20)), Health::NoResponse);
    }

    #[test]
    fn idle_tunnel_is_not_reported_as_broken() {
        let mut watch = HealthWatch::default();
        assert_eq!(run(&mut watch, 30, 0, 0, (5, 5)), Health::Unknown);
    }

    #[test]
    fn recovery_and_tunnel_switch() {
        let mut watch = HealthWatch::default();
        assert_eq!(run(&mut watch, 20, 0, 1, (0, 0)), Health::NoResponse);
        assert_eq!(watch.record("wg0", 1, 30), Health::Ok);
        // Another tunnel starts from scratch.
        assert_eq!(watch.record("other", 0, 100), Health::Unknown);
    }
}
