// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Tunnel traffic measurement and the live graph.
//!
//! Counters come from `/sys/class/net/<interface>/statistics`, readable without
//! any privilege. The WireGuard interface is created when the tunnel connects,
//! so these counters are also the totals since the connection started.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::f64::consts::PI;
use std::rc::Rc;

use adw::prelude::*;
use gtk::cairo;

/// Number of points shown: one minute at one sample per second.
const POINTS: usize = 60;

#[derive(Debug, Clone, Copy, Default)]
pub struct Sample {
    /// Bytes received per second.
    pub rx: f64,
    /// Bytes sent per second.
    pub tx: f64,
}

/// Cumulative counters of a tunnel interface since it was created.
#[derive(Debug, Clone, Copy, Default)]
pub struct Counters {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
}

pub fn read_counters(interface: &str) -> Option<Counters> {
    let read = |name: &str| {
        let path = format!("/sys/class/net/{interface}/statistics/{name}");
        std::fs::read_to_string(path)
            .ok()?
            .trim()
            .parse::<u64>()
            .ok()
    };
    Some(Counters {
        rx_bytes: read("rx_bytes")?,
        tx_bytes: read("tx_bytes")?,
        rx_packets: read("rx_packets")?,
        tx_packets: read("tx_packets")?,
    })
}

/// (download, upload) colours from the GNOME palette, contrast ≥ 3:1 on the background.
pub fn series_colors() -> ([f64; 3], [f64; 3]) {
    if adw::StyleManager::default().is_dark() {
        (rgb(0x62a0ea), rgb(0xffa348))
    } else {
        (rgb(0x1c71d8), rgb(0xc64600))
    }
}

pub fn hex_color(color: [f64; 3]) -> String {
    let [r, g, b] = color.map(|c| (c * 255.0).round() as u8);
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn rgb(hex: u32) -> [f64; 3] {
    [(hex >> 16) & 0xff, (hex >> 8) & 0xff, hex & 0xff].map(|c| f64::from(c) / 255.0)
}

#[derive(Clone)]
pub struct TrafficGraph {
    area: gtk::DrawingArea,
    history: Rc<RefCell<VecDeque<Sample>>>,
}

impl TrafficGraph {
    pub fn new() -> Self {
        let area = gtk::DrawingArea::builder()
            .content_height(150)
            .hexpand(true)
            .build();
        let history = Rc::new(RefCell::new(VecDeque::from(vec![
            Sample::default();
            POINTS
        ])));

        let data = history.clone();
        area.set_draw_func(move |area, cr, width, height| {
            draw(
                area,
                cr,
                f64::from(width),
                f64::from(height),
                &data.borrow(),
            );
        });
        // Colours follow the light/dark style.
        let weak = area.downgrade();
        adw::StyleManager::default().connect_dark_notify(move |_| {
            if let Some(area) = weak.upgrade() {
                area.queue_draw();
            }
        });
        Self { area, history }
    }

    pub fn widget(&self) -> &gtk::DrawingArea {
        &self.area
    }

    pub fn push(&self, sample: Sample) {
        let mut history = self.history.borrow_mut();
        history.pop_front();
        history.push_back(sample);
        drop(history);
        self.area.queue_draw();
    }

    pub fn clear(&self) {
        self.history
            .borrow_mut()
            .iter_mut()
            .for_each(|s| *s = Sample::default());
        self.area.queue_draw();
    }

    /// Top of the scale, in bytes per second.
    pub fn scale(&self) -> f64 {
        scale_for(&self.history.borrow())
    }

    /// The `count` most recent samples, oldest first.
    pub fn recent(&self, count: usize) -> Vec<Sample> {
        let history = self.history.borrow();
        history
            .iter()
            .skip(history.len().saturating_sub(count))
            .copied()
            .collect()
    }
}

/// Text sparkline (`▁▂▄▇`) for places that only show text, like the top bar menu.
pub fn sparkline(values: &[f64]) -> String {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let peak = values.iter().copied().fold(0.0, f64::max);
    values
        .iter()
        .map(|v| {
            let level = if peak > 0.0 { v / peak * 7.0 } else { 0.0 };
            BARS[level.round().clamp(0.0, 7.0) as usize]
        })
        .collect()
}

fn scale_for(history: &VecDeque<Sample>) -> f64 {
    let peak = history.iter().map(|s| s.rx.max(s.tx)).fold(0.0, f64::max);
    nice_ceiling(peak.max(1024.0))
}

/// Rounds up to the next 1, 2 or 5 × 10ⁿ step for a readable scale.
fn nice_ceiling(value: f64) -> f64 {
    let magnitude = 10f64.powf(value.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .find(|s| s * magnitude >= value)
        .unwrap_or(10.0);
    step * magnitude
}

fn draw(
    area: &gtk::DrawingArea,
    cr: &cairo::Context,
    width: f64,
    height: f64,
    history: &VecDeque<Sample>,
) {
    let fg = area.color();
    let (fg_r, fg_g, fg_b) = (
        f64::from(fg.red()),
        f64::from(fg.green()),
        f64::from(fg.blue()),
    );
    let scale = scale_for(history);
    let top = 6.0;
    let usable = height - top - 1.0;

    // Subtle grid: four horizontal bands.
    cr.set_line_width(1.0);
    cr.set_source_rgba(fg_r, fg_g, fg_b, 0.10);
    for i in 0..=4 {
        let y = (top + usable * f64::from(i) / 4.0).round() + 0.5;
        cr.move_to(0.0, y);
        cr.line_to(width, y);
    }
    log_cairo(cr.stroke());

    let (rx_color, tx_color) = series_colors();
    let points = |pick: fn(&Sample) -> f64| -> Vec<(f64, f64)> {
        history
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let x = width * i as f64 / (POINTS - 1) as f64;
                let y = top + usable * (1.0 - (pick(s) / scale).min(1.0));
                (x, y)
            })
            .collect()
    };
    draw_series(cr, &points(|s| s.tx), tx_color, height);
    draw_series(cr, &points(|s| s.rx), rx_color, height);
}

fn draw_series(cr: &cairo::Context, points: &[(f64, f64)], [r, g, b]: [f64; 3], height: f64) {
    let Some(&(x0, y0)) = points.first() else {
        return;
    };
    let Some(&(x_end, y_end)) = points.last() else {
        return;
    };

    // Smoothed curve: each segment is a Bézier curve with control points at the midpoints.
    let trace = |cr: &cairo::Context| {
        cr.move_to(x0, y0);
        for pair in points.windows(2) {
            let ((xa, ya), (xb, yb)) = (pair[0], pair[1]);
            let mid = (xa + xb) / 2.0;
            cr.curve_to(mid, ya, mid, yb, xb, yb);
        }
    };

    trace(cr);
    cr.line_to(x_end, height);
    cr.line_to(x0, height);
    cr.close_path();
    let gradient = cairo::LinearGradient::new(0.0, 0.0, 0.0, height);
    gradient.add_color_stop_rgba(0.0, r, g, b, 0.30);
    gradient.add_color_stop_rgba(1.0, r, g, b, 0.02);
    log_cairo(cr.set_source(&gradient));
    log_cairo(cr.fill());

    trace(cr);
    cr.set_source_rgb(r, g, b);
    cr.set_line_width(2.0);
    cr.set_line_join(cairo::LineJoin::Round);
    log_cairo(cr.stroke());

    // Dot on the latest sample.
    cr.arc(x_end - 3.0, y_end, 3.0, 0.0, 2.0 * PI);
    log_cairo(cr.fill());
}

/// Drawing errors cannot be shown to the user; they are logged and the frame is skipped.
fn log_cairo(result: Result<(), cairo::Error>) {
    if let Err(err) = result {
        gtk::glib::g_warning!(crate::LOG_DOMAIN, "Cannot draw traffic graph: {err}");
    }
}
