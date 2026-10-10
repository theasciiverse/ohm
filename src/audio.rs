//! All sound is synthesised here; Ohm's Workshop ships no audio files. The approach and the
//! helpers come from SIGPIPE (`games/sigpipe/src/audio.rs`), which took them from Panzer.
//!
//! - Effects are rendered once at start-up into sample buffers.
//! - One endless source, the current, plays while current flows: a soft mains-like hum with a
//!   faint buzz on top, louder the more current runs, quiet when nothing does.
//!
//! Without an audio device, or with `--mute`, every call does nothing.

use rodio::buffer::SamplesBuffer;
use rodio::{MixerDeviceSink, Source};
use std::f32::consts::TAU;
use std::num::NonZero;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SR: u32 = 44_100;
const SRF: f32 = SR as f32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sfx {
    /// A part pressed into the board.
    Place,
    /// A part pulled out, or copper scraped off.
    Erase,
    /// Copper laid on one tile.
    Lay,
    /// A switch flipped, a button pressed.
    Click,
    /// Something burnt out.
    Pop,
    /// Every case passed: the till.
    Pass,
    Fail,
    Select,
    Denied,
    /// A relay pulling in: a solid clack.
    RelayIn,
    /// A relay letting go: a lighter click as the spring takes the contact back.
    RelayOut,
}

const ALL_SFX: [Sfx; 11] =
    [Sfx::Place, Sfx::Erase, Sfx::Lay, Sfx::Click, Sfx::Pop, Sfx::Pass, Sfx::Fail, Sfx::Select, Sfx::Denied, Sfx::RelayIn, Sfx::RelayOut];

#[derive(Clone, Copy, Default)]
struct Live {
    /// Total current flowing, amps.
    amps: f32,
    /// A relay buzzing on its own contact, how fast; 0 for none.
    buzz: f32,
    silent: bool,
}

pub struct Audio {
    // the sink must outlive everything playing on its mixer
    sink: Option<MixerDeviceSink>,
    bank: Vec<SamplesBuffer>,
    live: Arc<Mutex<Live>>,
    pub muted: bool,
    last: [Option<Instant>; ALL_SFX.len()],
}

impl Audio {
    pub fn new(muted: bool) -> Audio {
        let sink = if muted {
            None
        } else {
            match rodio::DeviceSinkBuilder::open_default_sink() {
                Ok(mut s) => {
                    s.log_on_drop(false);
                    Some(s)
                }
                Err(e) => {
                    log::warn!("no audio: {e}");
                    None
                }
            }
        };
        let live = Arc::new(Mutex::new(Live::default()));
        let bank = if sink.is_some() { ALL_SFX.iter().map(|&s| buffer(render_sfx(s))).collect() } else { Vec::new() };
        if let Some(s) = &sink {
            s.mixer().add(Current::new(live.clone()));
        }
        Audio { sink, bank, live, muted, last: [None; ALL_SFX.len()] }
    }

    pub fn play(&mut self, sfx: Sfx) {
        if self.muted {
            return;
        }
        let Some(sink) = &self.sink else { return };
        let i = ALL_SFX.iter().position(|&s| s == sfx).unwrap_or(0);
        let now = Instant::now();
        if self.last[i].is_some_and(|t| now.duration_since(t) < Duration::from_millis(35)) {
            return;
        }
        self.last[i] = Some(now);
        sink.mixer().add(self.bank[i].clone());
    }

    /// How much current is flowing, for the hum.
    pub fn set_current(&mut self, amps: f64) {
        if let Ok(mut l) = self.live.lock() {
            l.amps = amps as f32;
        }
    }

    /// A relay chattering this many times a second, heard as a buzz; 0 stops it.
    pub fn set_buzz(&mut self, hz: f64) {
        if let Ok(mut l) = self.live.lock() {
            l.buzz = hz as f32;
        }
    }

    pub fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        if let Ok(mut l) = self.live.lock() {
            l.silent = self.muted;
        }
    }
}

fn buffer(samples: Vec<f32>) -> SamplesBuffer {
    SamplesBuffer::new(NonZero::new(1).unwrap(), NonZero::new(SR).unwrap(), samples)
}

// ------------------------------------------------------------------ helpers

struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

fn midi(n: f32) -> f32 {
    440.0 * 2f32.powf((n - 69.0) / 12.0)
}

fn square(ph: f32) -> f32 {
    if ph - ph.floor() < 0.5 { 1.0 } else { -1.0 }
}

fn tri(ph: f32) -> f32 {
    let p = ph - ph.floor();
    4.0 * (p - 0.5).abs() - 1.0
}

/// `dur` seconds of samples from `f(t, noise)`.
fn synth(dur: f32, mut f: impl FnMut(f32, &mut Noise) -> f32) -> Vec<f32> {
    let mut n = Noise(0x1234_5678);
    (0..(dur * SRF) as usize).map(|i| f(i as f32 / SRF, &mut n)).collect()
}

/// A little tune: (midi note, seconds), played on `wave` with a soft attack.
fn tune(notes: &[(f32, f32)], gain: f32, wave: fn(f32) -> f32) -> Vec<f32> {
    let mut out = Vec::new();
    let mut ph = 0.0f32;
    let mut lp = 0.0f32;
    for &(n, d) in notes {
        let len = (d * SRF) as usize;
        for i in 0..len {
            let t = i as f32 / SRF;
            if n > 0.0 {
                ph += midi(n) / SRF;
            }
            let env = (t / 0.008).min(1.0) * ((d - t) / 0.03).clamp(0.0, 1.0);
            let x = if n > 0.0 { wave(ph) * env } else { 0.0 };
            lp += (x - lp) * 0.3;
            out.push(lp * gain);
        }
    }
    out
}

/// Several sounds played together.
fn chord(parts: &[Vec<f32>]) -> Vec<f32> {
    let len = parts.iter().map(Vec::len).max().unwrap_or(0);
    (0..len).map(|i| parts.iter().map(|p| p.get(i).copied().unwrap_or(0.0)).sum()).collect()
}

fn render_sfx(s: Sfx) -> Vec<f32> {
    match s {
        // a component's legs pushed through the board: a dull thock and a tick
        Sfx::Place => synth(0.07, |t, n| {
            let thock = (TAU * 180.0 * t).sin() * (-t * 60.0).exp();
            let tick = n.next() * (-t * 300.0).exp();
            (thock * 0.5 + tick * 0.3) * 0.6
        }),
        // pulled out: a scrape falling in pitch
        Sfx::Erase => {
            let mut ph = 0.0f32;
            synth(0.12, |t, n| {
                ph += (900.0 - 5000.0 * t) / SRF;
                ((ph * TAU).sin() * 0.3 + n.next() * 0.4) * (1.0 - t / 0.12) * 0.25
            })
        }
        // a soft copper tick
        Sfx::Lay => synth(0.025, |t, _| (TAU * 3200.0 * t).sin() * (-t * 220.0).exp() * 0.15),
        // a toggle switch: two clicks close together
        Sfx::Click => synth(0.05, |t, n| {
            let a = (-t * 400.0).exp();
            let b = if t > 0.018 { (-(t - 0.018) * 500.0).exp() * 0.6 } else { 0.0 };
            (n.next() * 0.6 + (TAU * 2400.0 * t).sin() * 0.4) * (a + b) * 0.35
        }),
        // burnt out: a crack and a sizzle
        Sfx::Pop => {
            let crack = synth(0.05, |t, n| n.next() * (-t * 90.0).exp() * 0.7);
            let sizzle = synth(0.7, |t, n| n.next() * (t / 0.02).min(1.0) * (-t * 4.0).exp() * 0.12);
            chord(&[crack, sizzle])
        }
        // the till: a bell and a rising chord
        Sfx::Pass => chord(&[
            synth(0.6, |t, _| ((TAU * 2093.0 * t).sin() + (TAU * 2637.0 * t).sin() * 0.5) * (-t * 7.0).exp() * 0.12),
            tune(&[(0.0, 0.12), (72.0, 0.1), (76.0, 0.1), (79.0, 0.1), (84.0, 0.35)], 0.18, tri),
        ]),
        Sfx::Fail => tune(&[(64.0, 0.16), (60.0, 0.16), (55.0, 0.4)], 0.16, square),
        Sfx::Select => tune(&[(84.0, 0.04), (91.0, 0.06)], 0.12, square),
        Sfx::Denied => synth(0.12, |t, _| square(95.0 * t) * (1.0 - t / 0.12) * 0.12),
        // the armature slapping onto the core: a thud, a sharp tick and a short metal ring
        Sfx::RelayIn => synth(0.06, |t, n| {
            let thud = (TAU * 140.0 * t).sin() * (-t * 80.0).exp() * 0.5;
            let tick = n.next() * (-t * 700.0).exp() * 0.6;
            let ring = (TAU * 3100.0 * t).sin() * (-t * 120.0).exp() * 0.2;
            (thud + tick + ring) * 0.45
        }),
        Sfx::RelayOut => synth(0.04, |t, n| {
            let tick = n.next() * (-t * 900.0).exp() * 0.5;
            let ring = (TAU * 2600.0 * t).sin() * (-t * 160.0).exp() * 0.15;
            (tick + ring) * 0.35
        }),
    }
}

// ------------------------------------------------------------------ the current

/// The sound of current: a 50 Hz hum and a faint buzz, rendered in blocks.
struct Current {
    live: Arc<Mutex<Live>>,
    buf: Vec<f32>,
    pos: usize,
    noise: Noise,
    hum: f32,
    level: f32,
    lp: f32,
    /// The buzzing relay's phase and loudness.
    rattle: f32,
    rattle_level: f32,
}

const BLOCK: usize = 512;

impl Current {
    fn new(live: Arc<Mutex<Live>>) -> Current {
        Current { live, buf: vec![0.0; BLOCK], pos: BLOCK, noise: Noise(0xBEEF), hum: 0.0, level: 0.0, lp: 0.0, rattle: 0.0, rattle_level: 0.0 }
    }

    fn render(&mut self) {
        let live = self.live.lock().map(|l| *l).unwrap_or_default();
        // 0.1 mA is a whisper, 100 mA and up is full.
        let target = if live.silent || live.amps < 1e-4 { 0.0 } else { ((live.amps / 1e-4).log10() / 3.0).clamp(0.15, 1.0) };
        for i in 0..BLOCK {
            self.level += (target - self.level) * 0.0005;
            self.hum += 50.0 / SRF;
            let hum = (self.hum * TAU).sin() * 0.6 + (self.hum * 2.0 * TAU).sin() * 0.3 + (self.hum * 3.0 * TAU).sin() * 0.1;
            let buzz = self.noise.next() * 0.08 * square(self.hum * 2.0).max(0.0);
            // A buzzing relay: the armature hitting the core once a cycle, a raspy pulse.
            let want = if live.silent || live.buzz <= 0.0 { 0.0 } else { 1.0 };
            self.rattle_level += (want - self.rattle_level) * 0.002;
            if live.buzz > 0.0 {
                self.rattle += live.buzz / SRF;
            }
            let p = self.rattle - self.rattle.floor();
            let rattle = ((-p * 30.0).exp() * 0.8 + self.noise.next() * (-p * 12.0).exp() * 0.4) * self.rattle_level;
            let x = (hum * 0.06 + buzz * 0.05) * self.level + rattle * 0.12;
            self.lp += (x - self.lp) * 0.3;
            self.buf[i] = self.lp;
        }
    }
}

impl Iterator for Current {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.pos >= self.buf.len() {
            self.render();
            self.pos = 0;
        }
        self.pos += 1;
        Some(self.buf[self.pos - 1])
    }
}

impl Source for Current {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        NonZero::new(1).unwrap()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        NonZero::new(SR).unwrap()
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sounds_render_and_stay_in_range() {
        for s in ALL_SFX {
            let v = render_sfx(s);
            assert!(!v.is_empty(), "{s:?} is empty");
            let peak = v.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            assert!(v.iter().all(|x| x.is_finite()), "{s:?} not finite");
            assert!(peak > 0.01 && peak <= 1.0, "{s:?} peak {peak}");
        }
    }

    #[test]
    fn current_hums_while_flowing_and_hushes_after() {
        let live = Arc::new(Mutex::new(Live { amps: 0.012, buzz: 0.0, silent: false }));
        let mut s = Current::new(live.clone());
        let loud = (0..SR as usize * 3).map(|_| s.next().unwrap()).fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(loud > 0.005 && loud <= 1.0, "peak {loud}");
        live.lock().unwrap().amps = 0.0;
        let quiet: f32 = (0..SR as usize * 3).map(|_| s.next().unwrap()).skip(SR as usize * 2).fold(0.0, |m, x| m.max(x.abs()));
        assert!(quiet < 0.002, "still {quiet} after stopping");
    }
}
