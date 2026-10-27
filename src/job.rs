//! Jobs: a customer's gadget to build, read from the text files in `jobs/`.
//!
//! ```text
//! # comments
//! name: Bike lamp
//! customer: Mrs Berg, Storgatan
//! fee: 40                                   (kronor)
//! note: The bulb in my bike lamp ...        (one paragraph per line)
//! battery: 2850                             (mAh, for the battery-life score; 0 leaves it out)
//! book: leds, ohms-law                      (Notebook pages the job builds on)
//! part: B battery 4.5 0.45 N                (a grid character, a part, the side pin a faces)
//! drawer: resistor 220 0.25 x2 0.10         (a part, how many (x- for any), price in kronor)
//! case: LED 5-20 mA | battery 4.8 | lit LED1 5m 20m
//! life: LED 5-20 mA                         (the case whose current drains the battery, if any)
//! hint-parts: Four small resistors ...      (the master's word when you miss the parts tick)
//! hint-battery: The bigger the resistor ...  (and when you miss the battery tick)
//! master-part: R resistor 220 0.25 W        (a part in the master's own solution)
//! master: .B──R─L.                          (one row of his solution; the grid's parts too)
//! grid:
//! ..........
//! .B......L.
//! ```
//!
//! Parts: `resistor OHMS WATTS`, `lamp OHMS WATTS`, `battery VOLTS R_INT`, `supply VOLTS LIMIT`,
//! `switch`, `button`, `led COLOUR`, `cap FARADS`, `testpoint`. Numbers take SI suffixes (`5m`,
//! `100u`, `4k7`). Every part on the grid is fixed. A case is a name, then settings (`battery V`,
//! `set LABEL on|off|NUMBER`, `at SECONDS set LABEL ...` to change it partway), then one check
//! (`lit LABEL MIN MAX`, `dark LABEL`, `volts LABEL MIN MAX`, `blown LABEL`; each may end in
//! `at SECONDS`, when it reads), or a timed check counted from the last `at` (`lasts LABEL MIN
//! MAX`: stays lit that many seconds; `reaches LABEL VOLTS MIN MAX`: gets to that voltage).
//! `cap FARADS [VOLTS]`: from 1 µF an electrolytic, + facing the side given, rated VOLTS (25).
//! `stop-button`: closed until pressed. `relay VOLTS [nc]`: its coil's legs on the side given and
//! its opposite, the contact's across; `nc` for a contact closed at rest. `blinks LABEL MIN MAX`:
//! an LED or a lamp blinks, or a relay chatters, MIN to MAX times a second. `diode`: a 1N4001,
//! anode on the side given. `npn [flip]`: a BC547B, collector on the side given, base a quarter
//! turn clockwise (anticlockwise with `flip`).

use crate::bench::{Case, Check};
use crate::circuit::{Board, Chip, Colour, Dir, Kind};

/// The jobs, in order, compiled in.
pub const JOBS: &[(&str, &str)] = &[
    ("bike-lamp", include_str!("../jobs/01-bike-lamp.txt")),
    ("porch-lamp", include_str!("../jobs/02-porch-lamp.txt")),
    ("two-doors", include_str!("../jobs/03-two-doors.txt")),
    ("dolls-house", include_str!("../jobs/04-dolls-house.txt")),
    ("christmas-star", include_str!("../jobs/05-christmas-star.txt")),
    ("cafe-sign", include_str!("../jobs/06-cafe-sign.txt")),
    ("boat-gauge", include_str!("../jobs/07-boat-gauge.txt")),
    ("night-dimmer", include_str!("../jobs/08-night-dimmer.txt")),
    ("fuel-gauge", include_str!("../jobs/09-fuel-gauge.txt")),
    ("light-meter", include_str!("../jobs/10-light-meter.txt")),
    ("fridge-gauge", include_str!("../jobs/11-fridge-gauge.txt")),
    ("trailer-fuse", include_str!("../jobs/12-trailer-fuse.txt")),
    ("standlight", include_str!("../jobs/13-standlight.txt")),
    ("courtesy-light", include_str!("../jobs/14-courtesy-light.txt")),
    ("darkroom-sign", include_str!("../jobs/15-darkroom-sign.txt")),
    ("level-crossing", include_str!("../jobs/16-level-crossing.txt")),
    ("crossing-lifts", include_str!("../jobs/17-crossing-lifts.txt")),
    ("puppet-theatre", include_str!("../jobs/18-puppet-theatre.txt")),
    ("spotlamps", include_str!("../jobs/19-spotlamps.txt")),
    ("coffee-roaster", include_str!("../jobs/20-coffee-roaster.txt")),
    ("railway-signal", include_str!("../jobs/21-railway-signal.txt")),
    ("doorbell-buzzer", include_str!("../jobs/22-doorbell-buzzer.txt")),
    ("cellar-light", include_str!("../jobs/23-cellar-light.txt")),
    ("lighthouse", include_str!("../jobs/24-lighthouse.txt")),
    ("flash-battery", include_str!("../jobs/25-flash-battery.txt")),
    ("which-door", include_str!("../jobs/26-which-door.txt")),
    ("flood-alarm", include_str!("../jobs/27-flood-alarm.txt")),
    ("oil-warning", include_str!("../jobs/28-oil-warning.txt")),
    ("freezer-alarm", include_str!("../jobs/29-freezer-alarm.txt")),
    ("jetty-light", include_str!("../jobs/30-jetty-light.txt")),
    ("occupied-sign", include_str!("../jobs/31-occupied-sign.txt")),
    ("crossing-lights", include_str!("../jobs/32-crossing-lights.txt")),
    ("quiz-timer", include_str!("../jobs/33-quiz-timer.txt")),
    ("boathouse-alarm", include_str!("../jobs/34-boathouse-alarm.txt")),
];

/// The sandbox's id, for the save file.
pub const SANDBOX: &str = "sandbox";

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub kind: Kind,
    /// How many are left; `None` for as many as you like.
    pub count: Option<u32>,
    /// Kronor each.
    pub price: f64,
}

#[derive(Clone, Debug)]
pub struct Job {
    pub id: String,
    pub name: String,
    pub customer: String,
    pub fee: f64,
    pub note: Vec<String>,
    /// Battery capacity in mAh, for the battery-life score (0: don't say).
    pub battery_mah: f64,
    /// Notebook pages the job builds on, by id.
    pub book: Vec<String>,
    pub board: Board,
    pub drawer: Vec<Item>,
    pub cases: Vec<Case>,
    /// The case whose current counts for battery life; `None` when it doesn't matter.
    pub life: Option<String>,
    /// The old master's own solution: the par for parts cost and battery life.
    pub master: Board,
    /// The master's word when a build misses the parts tick, or the battery tick.
    pub hint_parts: Option<String>,
    pub hint_battery: Option<String>,
}

/// How a build scores: what its parts cost, and how long the battery lasts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Score {
    /// Kronor, for the parts from the drawers.
    pub cost: f64,
    /// Hours on a fresh battery, for jobs where that counts.
    pub hours: Option<f64>,
}

/// The three ticks a job can earn: it works; its parts cost no more than the master's; its
/// battery lasts as long as his.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ticks {
    pub works: bool,
    pub thrifty: bool,
    pub frugal: bool,
}

impl Ticks {
    /// The better of two: each tick, once earned, stays.
    pub fn or(self, o: Ticks) -> Ticks {
        Ticks { works: self.works || o.works, thrifty: self.thrifty || o.thrifty, frugal: self.frugal || o.frugal }
    }
}

/// The same part for the drawers: a switch is a switch whether it's on or off.
pub fn same_part(a: Kind, b: Kind) -> bool {
    let norm = |k: Kind| match k {
        Kind::Switch { .. } => Kind::Switch { closed: true },
        Kind::Button { nc, .. } => Kind::Button { held: false, nc },
        // A sensor's light or warmth and a pot's turn are where it is, not what it is.
        Kind::Ldr { .. } => Kind::Ldr { lux: 0.0 },
        Kind::Ntc { .. } => Kind::Ntc { celsius: 0.0 },
        Kind::Pot { ohms, .. } => Kind::Pot { ohms, turn: 0.0 },
        Kind::Npn { .. } => Kind::Npn { flip: false },
        k => k,
    };
    norm(a) == norm(b)
}

/// A number with an optional SI suffix: `220`, `4k7`, `5m`, `100u`, `0.25`.
pub fn number(s: &str) -> Result<f64, String> {
    let scale = |c: char| match c {
        'p' => Some(1e-12),
        'n' => Some(1e-9),
        'u' | 'µ' => Some(1e-6),
        'm' => Some(1e-3),
        'k' => Some(1e3),
        'M' => Some(1e6),
        _ => None,
    };
    if let Ok(v) = s.parse() {
        return Ok(v);
    }
    let (i, c) = s.char_indices().find(|(_, c)| scale(*c).is_some()).ok_or(format!("not a number: {s}"))?;
    let whole = &s[..i];
    let frac = &s[i + c.len_utf8()..];
    let digits = if frac.is_empty() { whole.to_string() } else { format!("{whole}.{frac}") };
    let v: f64 = digits.parse().map_err(|_| format!("not a number: {s}"))?;
    Ok(v * scale(c).unwrap())
}

/// A part from words: `resistor 220 0.25`, `led red`.
pub fn kind(words: &[&str]) -> Result<(Kind, usize), String> {
    let arg = |i: usize| words.get(i).ok_or(format!("{} needs more values", words[0])).and_then(|w| number(w));
    Ok(match words.first().copied() {
        Some("resistor") => (Kind::Resistor { ohms: arg(1)?, watts: arg(2)? }, 3),
        Some("lamp") => (Kind::Lamp { ohms: arg(1)?, watts: arg(2)? }, 3),
        Some("battery") => (Kind::Battery { volts: arg(1)?, r_int: arg(2)? }, 3),
        Some("supply") => (Kind::Supply { volts: arg(1)?, limit: arg(2)? }, 3),
        Some("switch") => match words.get(1).copied() {
            Some("off") => (Kind::Switch { closed: false }, 2),
            Some("on") => (Kind::Switch { closed: true }, 2),
            _ => (Kind::Switch { closed: true }, 1),
        },
        Some("button") => (Kind::Button { held: false, nc: false }, 1),
        Some("stop-button") => (Kind::Button { held: false, nc: true }, 1),
        // An electrolytic's rating is optional (25 V); a ceramic below 1 µF has none.
        Some("cap") => {
            let farads = arg(1)?;
            match words.get(2).map(|w| number(w)) {
                Some(Ok(volts)) => (Kind::Capacitor { farads, volts }, 3),
                _ => (Kind::Capacitor { farads, volts: if farads >= 1e-6 { 25.0 } else { f64::INFINITY } }, 2),
            }
        }
        Some("testpoint") => (Kind::TestPoint, 1),
        Some("fuse") => (Kind::Fuse { amps: arg(1)? }, 2),
        // Sensors and pots take their state as an optional last word.
        Some("ldr") => match words.get(1).map(|w| number(w)) {
            Some(Ok(lux)) => (Kind::Ldr { lux }, 2),
            _ => (Kind::Ldr { lux: 10.0 }, 1),
        },
        Some("ntc") => match words.get(1).map(|w| number(w)) {
            Some(Ok(celsius)) => (Kind::Ntc { celsius }, 2),
            _ => (Kind::Ntc { celsius: 20.0 }, 1),
        },
        Some("pot") => match words.get(2).map(|w| number(w)) {
            Some(Ok(turn)) => (Kind::Pot { ohms: arg(1)?, turn }, 3),
            _ => (Kind::Pot { ohms: arg(1)?, turn: 0.5 }, 2),
        },
        Some("diode") => (Kind::Diode, 1),
        Some("cd4011") => (Kind::Chip(Chip::Cd4011), 1),
        Some("npn") => match words.get(1).copied() {
            Some("flip") => (Kind::Npn { flip: true }, 2),
            _ => (Kind::Npn { flip: false }, 1),
        },
        // A relay by its coil's voltage, normally open unless `nc` follows.
        Some("relay") => match words.get(2).copied() {
            Some("nc") => (Kind::Relay { volts: arg(1)?, nc: true }, 3),
            _ => (Kind::Relay { volts: arg(1)?, nc: false }, 2),
        },
        Some("led") => {
            let c = match words.get(1).copied() {
                Some("red") => Colour::Red,
                Some("yellow") => Colour::Yellow,
                Some("green") => Colour::Green,
                Some("blue") => Colour::Blue,
                other => return Err(format!("no LED colour {other:?}")),
            };
            (Kind::Led(c), 2)
        }
        other => return Err(format!("no part called {other:?}")),
    })
}

/// A part as words, the way `kind` reads them back: `resistor 220 0.25`, `led red`.
pub fn kind_words(k: Kind) -> String {
    match k {
        Kind::Resistor { ohms, watts } => format!("resistor {ohms} {watts}"),
        Kind::Lamp { ohms, watts } => format!("lamp {ohms} {watts}"),
        Kind::Battery { volts, r_int } => format!("battery {volts} {r_int}"),
        Kind::Supply { volts, limit } => format!("supply {volts} {limit}"),
        Kind::Switch { closed } => format!("switch {}", if closed { "on" } else { "off" }),
        Kind::Button { nc: false, .. } => "button".into(),
        Kind::Button { nc: true, .. } => "stop-button".into(),
        Kind::Capacitor { farads, volts } if volts.is_finite() => format!("cap {farads} {volts}"),
        Kind::Capacitor { farads, .. } => format!("cap {farads}"),
        Kind::TestPoint => "testpoint".into(),
        Kind::Led(c) => format!("led {}", c.name()),
        Kind::Fuse { amps } => format!("fuse {amps}"),
        Kind::Ldr { lux } => format!("ldr {lux}"),
        Kind::Ntc { celsius } => format!("ntc {celsius}"),
        Kind::Pot { ohms, turn } => format!("pot {ohms} {turn}"),
        Kind::Relay { volts, nc } => format!("relay {volts}{}", if nc { " nc" } else { "" }),
        Kind::Diode => "diode".into(),
        Kind::Npn { flip } => if flip { "npn flip".into() } else { "npn".into() },
        Kind::Chip(Chip::Cd4011) => "cd4011".into(),
    }
}

pub fn dir_name(d: Dir) -> &'static str {
    match d {
        Dir::N => "N",
        Dir::E => "E",
        Dir::S => "S",
        Dir::W => "W",
    }
}

pub fn dir(w: &str) -> Result<Dir, String> {
    Ok(match w {
        "N" => Dir::N,
        "E" => Dir::E,
        "S" => Dir::S,
        "W" => Dir::W,
        _ => return Err(format!("not a side: {w}")),
    })
}

fn case(line: &str) -> Result<Case, String> {
    let mut parts = line.split('|').map(str::trim);
    let name = parts.next().unwrap_or_default().to_string();
    let mut c = Case::steady(&name, None, Vec::new(), Check::Dark { part: String::new() });
    let mut checked = false;
    let state = |s: &str| -> Result<f64, String> {
        match s {
            "on" => Ok(1.0),
            "off" => Ok(0.0),
            n => number(n),
        }
    };
    for p in parts {
        let mut w: Vec<&str> = p.split_whitespace().collect();
        // A steady check can say when it reads: `lit LED1 5m 20m at 3`.
        if w.len() > 2 && w[w.len() - 2] == "at" && w[0] != "at" {
            c.at = Some(number(w[w.len() - 1])?);
            w.truncate(w.len() - 2);
        }
        match w.as_slice() {
            ["battery", v] => c.battery = Some(number(v)?),
            ["set", label, s] => c.set.push((label.to_string(), state(s)?)),
            ["at", t, "set", label, s] => {
                c.events.push((number(t)?, label.to_string(), state(s)?));
                c.events.sort_by(|a, b| a.0.total_cmp(&b.0));
            }
            ["lasts", label, min, max] => {
                c.check = Check::Lasts { part: label.to_string(), min: number(min)?, max: number(max)? };
                checked = true;
            }
            ["reaches", label, v, min, max] => {
                c.check = Check::Reaches { point: label.to_string(), volts: number(v)?, min: number(min)?, max: number(max)? };
                checked = true;
            }
            ["blinks", label, min, max] => {
                c.check = Check::Blinks { part: label.to_string(), min: number(min)?, max: number(max)? };
                checked = true;
            }
            ["lit", label, min, max] => {
                c.check = Check::Lit { part: label.to_string(), min: number(min)?, max: number(max)? };
                checked = true;
            }
            ["dark", label] => {
                c.check = Check::Dark { part: label.to_string() };
                checked = true;
            }
            ["blown", label] => {
                c.check = Check::Blown { part: label.to_string() };
                checked = true;
            }
            ["volts", label, min, max] => {
                c.check = Check::Volts { point: label.to_string(), min: number(min)?, max: number(max)? };
                checked = true;
            }
            _ => return Err(format!("can't read case part {p:?}")),
        }
    }
    if !checked {
        return Err(format!("case {:?} checks nothing", c.name));
    }
    Ok(c)
}

impl Job {
    pub fn parse(id: &str, text: &str) -> Result<Job, String> {
        let mut job = Job {
            id: id.to_string(),
            name: String::new(),
            customer: String::new(),
            fee: 0.0,
            note: Vec::new(),
            battery_mah: 2000.0,
            book: Vec::new(),
            board: Board::new(1, 1),
            drawer: Vec::new(),
            cases: Vec::new(),
            life: None,
            master: Board::new(1, 1),
            hint_parts: None,
            hint_battery: None,
        };
        let mut legend = Vec::new();
        let mut master_legend = Vec::new();
        let mut master: Vec<String> = Vec::new();
        let mut grid: Option<Vec<&str>> = None;
        for (n, line) in text.lines().enumerate() {
            let err = |e: String| format!("{id} line {}: {e}", n + 1);
            if let Some(rows) = &mut grid {
                if !line.trim().is_empty() {
                    rows.push(line);
                }
                continue;
            }
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line == "grid:" {
                grid = Some(Vec::new());
                continue;
            }
            let (key, value) = line.split_once(':').ok_or_else(|| err(format!("no key in {line:?}")))?;
            let value = value.trim();
            match key {
                "name" => job.name = value.into(),
                "customer" => job.customer = value.into(),
                "fee" => job.fee = number(value).map_err(err)?,
                "note" => job.note.push(value.into()),
                "battery" => job.battery_mah = number(value).map_err(err)?,
                "book" => job.book = value.split(',').map(|s| s.trim().to_string()).collect(),
                "part" | "master-part" => {
                    let w: Vec<&str> = value.split_whitespace().collect();
                    let ch = w.first().and_then(|s| s.chars().next()).ok_or_else(|| err("part needs a character".into()))?;
                    let (k, used) = kind(&w[1..]).map_err(err)?;
                    let facing = dir(w.get(1 + used).copied().unwrap_or("W")).map_err(err)?;
                    if key == "part" { &mut legend } else { &mut master_legend }.push((ch, k, facing));
                }
                "master" => master.push(value.replace('.', " ")),
                "life" => job.life = Some(value.into()),
                "hint-parts" => job.hint_parts = Some(value.into()),
                "hint-battery" => job.hint_battery = Some(value.into()),
                "drawer" => {
                    let w: Vec<&str> = value.split_whitespace().collect();
                    let (k, used) = kind(&w).map_err(err)?;
                    let count = match w.get(used) {
                        Some(&"x-") => None,
                        Some(c) if c.starts_with('x') => Some(c[1..].parse().map_err(|_| err(format!("bad count {c}")))?),
                        _ => return Err(err("drawer needs a count like x2".into())),
                    };
                    let price = number(w.get(used + 1).copied().unwrap_or("0")).map_err(err)?;
                    job.drawer.push(Item { kind: k, count, price });
                }
                "case" => job.cases.push(case(value).map_err(err)?),
                _ => return Err(err(format!("unknown key {key:?}"))),
            }
        }
        let rows = grid.ok_or(format!("{id}: no grid"))?;
        let rows: Vec<String> = rows.iter().map(|r| r.replace('.', " ")).collect();
        let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
        job.board = Board::parse(&refs, &legend).map_err(|e| format!("{id}: {e}"))?;
        for p in &mut job.board.parts {
            p.fixed = true;
        }
        // The master's solution: the job's own parts where the job puts them, and his.
        if master.is_empty() {
            return Err(format!("{id}: no master solution"));
        }
        let all: Vec<(char, Kind, Dir)> = legend.iter().chain(&master_legend).copied().collect();
        let refs: Vec<&str> = master.iter().map(String::as_str).collect();
        job.master = Board::parse(&refs, &all).map_err(|e| format!("{id} master: {e}"))?;
        for p in &job.board.parts {
            let q = job.master.part(&p.label).map(|i| &job.master.parts[i]);
            if q.is_none_or(|q| (q.pos, q.facing) != (p.pos, p.facing)) {
                return Err(format!("{id} master: {} isn't where the job puts it", p.label));
            }
        }
        for p in &mut job.master.parts {
            p.fixed = job.board.part(&p.label).is_some();
        }
        if let Some(name) = &job.life
            && !job.cases.iter().any(|c| &c.name == name)
        {
            return Err(format!("{id}: life names no case {name:?}"));
        }
        Ok(job)
    }

    /// What a build scores: its parts' cost, and its battery life where that counts.
    pub fn score(&self, board: &Board) -> Score {
        let cost = board.parts.iter().filter(|p| !p.fixed).filter_map(|p| self.drawer.iter().find(|it| same_part(it.kind, p.kind)).map(|it| it.price)).sum();
        let hours = self.life.as_ref().and_then(|name| self.cases.iter().find(|c| &c.name == name)).and_then(|case| {
            let amps = crate::bench::drawn(board, case);
            (amps > 1e-6 && self.battery_mah > 0.0).then(|| self.battery_mah / (amps * 1000.0))
        });
        Score { cost, hours }
    }

    /// The master's score: the par.
    pub fn par(&self) -> Score {
        self.score(&self.master)
    }

    /// The ticks a build earns, judged against the master's par. `works` is the bench's verdict.
    pub fn ticks(&self, works: bool, mine: Score) -> Ticks {
        let par = self.par();
        Ticks {
            works,
            thrifty: works && mine.cost <= par.cost + 0.005,
            // Within a tenth of a percent: the same circuit gives the same hours.
            frugal: works && match (mine.hours, par.hours) {
                (Some(m), Some(p)) => m >= p * 0.999,
                (_, None) => true,
                (None, Some(_)) => false,
            },
        }
    }

    /// The sandbox: no customer, no spec and no fee, a bare stretch of workbench, and as many
    /// as you like of every kind of part, free.
    pub fn sandbox() -> Job {
        let r = |ohms: f64, watts: f64| Kind::Resistor { ohms, watts };
        let mut drawer = vec![
            // Batteries: AA cells at 0.15 ohm each (Energizer E91), the 9 V block and the van
            // battery as the jobs have them; a bench supply.
            Kind::Battery { volts: 1.5, r_int: 0.15 },
            Kind::Battery { volts: 4.5, r_int: 0.45 },
            Kind::Battery { volts: 9.0, r_int: 1.5 },
            Kind::Battery { volts: 12.0, r_int: 0.05 },
            Kind::Supply { volts: 5.0, limit: 0.1 },
            Kind::Led(Colour::Red),
            Kind::Led(Colour::Yellow),
            Kind::Led(Colour::Green),
            Kind::Led(Colour::Blue),
            Kind::Lamp { ohms: 15.0, watts: 1.35 },
            Kind::Lamp { ohms: 20.0, watts: 1.8 },
        ];
        // E12 values across the useful decades, and the café sign's power resistors.
        for ohms in [10.0, 22.0, 47.0, 100.0, 150.0, 220.0, 330.0, 470.0, 1e3, 2.2e3, 4.7e3, 10e3, 15e3, 22e3, 47e3, 100e3] {
            drawer.push(r(ohms, 0.25));
        }
        drawer.extend([r(100.0, 1.0), r(22.0, 2.0)]);
        // A ceramic, then electrolytics at the ratings they are commonly sold in, and one
        // 6.3 V can to find out what a rating means.
        drawer.push(Kind::Capacitor { farads: 100e-9, volts: f64::INFINITY });
        drawer.extend(
            [(10e-6, 25.0), (100e-6, 25.0), (470e-6, 16.0), (1000e-6, 16.0), (2200e-6, 16.0), (4700e-6, 16.0), (1000e-6, 6.3)]
                .map(|(farads, volts)| Kind::Capacitor { farads, volts }),
        );
        drawer.extend([Kind::Ldr { lux: 100.0 }, Kind::Ntc { celsius: 20.0 }, Kind::Pot { ohms: 10e3, turn: 0.5 }, Kind::Pot { ohms: 1e3, turn: 0.5 }]);
        drawer.extend([0.5, 2.0, 5.0].map(|amps| Kind::Fuse { amps }));
        drawer.extend([Kind::Switch { closed: false }, Kind::Button { held: false, nc: false }, Kind::Button { held: false, nc: true }]);
        drawer.extend([Kind::Diode, Kind::Npn { flip: false }, Kind::Chip(Chip::Cd4011)]);
        // Relays at the coil voltages the batteries give, normally open and normally closed.
        for volts in [5.0, 9.0, 12.0] {
            drawer.extend([Kind::Relay { volts, nc: false }, Kind::Relay { volts, nc: true }]);
        }
        Job {
            id: SANDBOX.into(),
            name: "Sandbox".into(),
            customer: "Your own bench".into(),
            fee: 0.0,
            note: vec![
                "No customer, no spec and no bill: the workbench, and as many as you like of every part in the shop, free.".into(),
                "Build anything, measure it, burn it and mend it.".into(),
            ],
            battery_mah: 0.0,
            book: Vec::new(),
            board: Board::new(16, 9),
            drawer: drawer.into_iter().map(|kind| Item { kind, count: None, price: 0.0 }).collect(),
            cases: Vec::new(),
            life: None,
            master: Board::new(16, 9),
            hint_parts: None,
            hint_battery: None,
        }
    }

    pub fn load(i: usize) -> Job {
        let (id, text) = JOBS[i];
        Job::parse(id, text).unwrap_or_else(|e| panic!("job {id} doesn't parse: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::run_case;
    use crate::circuit::Pos;

    #[test]
    fn every_masters_solution_passes_and_sets_the_par() {
        for i in 0..JOBS.len() {
            let job = Job::load(i);
            for c in &job.cases {
                let o = run_case(&job.master, c);
                assert!(o.pass, "{} master fails {}: {}", job.id, c.name, o.why);
            }
            let par = job.par();
            assert_eq!(job.ticks(true, par), Ticks { works: true, thrifty: true, frugal: true }, "{}", job.id);
            assert_eq!(par.hours.is_some(), job.life.is_some(), "{}", job.id);
            println!("{}: par {:.2} kr, {:?} h", job.id, par.cost, par.hours);
        }
        // The bike lamp: 220 ohm and a switch, 2.10 kr. 150 ohm passes too but drains the cells
        // faster, so it misses the battery tick.
        let job = Job::load(0);
        let par = job.par();
        assert!((par.cost - 2.10).abs() < 1e-9);
        let mut b = job.master.clone();
        let r = b.part("R1").unwrap();
        b.parts[r].kind = Kind::Resistor { ohms: 150.0, watts: 0.25 };
        assert!(job.cases.iter().all(|c| run_case(&b, c).pass));
        let mine = job.score(&b);
        assert!(mine.hours.unwrap() < par.hours.unwrap());
        assert_eq!(job.ticks(true, mine), Ticks { works: true, thrifty: true, frugal: false });
    }

    #[test]
    fn numbers_take_si_suffixes() {
        assert_eq!(number("220").unwrap(), 220.0);
        assert_eq!(number("4k7").unwrap(), 4700.0);
        assert!((number("5m").unwrap() - 0.005).abs() < 1e-12);
        assert!((number("100u").unwrap() - 1e-4).abs() < 1e-12);
        assert_eq!(number("1k").unwrap(), 1000.0);
        assert!(number("abc").is_err());
    }

    #[test]
    fn every_job_parses_and_points_at_real_pages() {
        let pages: Vec<String> = crate::book::Page::all().into_iter().map(|p| p.id).collect();
        for i in 0..JOBS.len() {
            let job = Job::load(i);
            assert!(!job.cases.is_empty(), "{} has no cases", job.id);
            assert!(job.board.parts.iter().all(|p| p.fixed));
            for b in &job.book {
                assert!(pages.contains(b), "{} points at page {b}, which doesn't exist", job.id);
            }
        }
    }

    /// Builds the bike lamp with a given resistor and runs its spec.
    fn bike_lamp_with(ohms: f64) -> Vec<bool> {
        let job = Job::load(0);
        let mut b = job.board.clone();
        let at = |x, y| Pos { x, y };
        // B at (1,2) facing N, L at (8,2) facing N: a loop over the top through R and back
        // along the bottom through a switch.
        b.place(Kind::Resistor { ohms, watts: 0.25 }, at(4, 1), Dir::W, false);
        b.place(Kind::Switch { closed: true }, at(4, 3), Dir::W, false);
        for (x, y, d) in [(1, 2, Dir::N), (1, 1, Dir::E), (2, 1, Dir::E), (3, 1, Dir::E), (4, 1, Dir::E), (5, 1, Dir::E), (6, 1, Dir::E), (7, 1, Dir::E), (8, 1, Dir::S)] {
            assert!(b.connect(at(x, y), d), "{x},{y} {d:?}");
        }
        for (x, y, d) in [(1, 2, Dir::S), (1, 3, Dir::E), (2, 3, Dir::E), (3, 3, Dir::E), (4, 3, Dir::E), (5, 3, Dir::E), (6, 3, Dir::E), (7, 3, Dir::E), (8, 3, Dir::N)] {
            assert!(b.connect(at(x, y), d), "{x},{y} {d:?}");
        }
        job.cases.iter().map(|c| run_case(&b, c).pass).collect()
    }

    /// Runs a job's cases on a whole solution drawn as text. The job's fixed parts must be
    /// where the job puts them; `legend` covers every part character in `rows`.
    fn solve(job: usize, rows: &[&str], legend: &[(char, Kind, Dir)]) -> Vec<bool> {
        let job = Job::load(job);
        let rows: Vec<String> = rows.iter().map(|r| r.replace('.', " ")).collect();
        let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
        let b = crate::circuit::Board::parse(&refs, legend).unwrap();
        for p in &job.board.parts {
            let q = &b.parts[b.part(&p.label).unwrap_or_else(|| panic!("{} missing", p.label))];
            assert_eq!((q.pos, q.facing), (p.pos, p.facing), "{} moved", p.label);
        }
        job.cases.iter().map(|c| run_case(&b, c).pass).collect()
    }

    const BAT45: Kind = Kind::Battery { volts: 4.5, r_int: 0.45 };
    const LAMP: Kind = Kind::Lamp { ohms: 15.0, watts: 1.35 };
    const BUTTON: Kind = Kind::Button { held: false, nc: false };
    const R: fn(f64) -> Kind = |ohms| Kind::Resistor { ohms, watts: 0.25 };

    const BAT12: Kind = Kind::Battery { volts: 12.0, r_int: 0.05 };
    const TP: Kind = Kind::TestPoint;

    /// A divider on the chapter 2 layout: `top` along the top, `bottom` down from the middle,
    /// the test point off the middle; `flip` swaps them.
    fn divider(job: usize, bat: Kind, top: Kind, bottom: Kind) -> Vec<bool> {
        let rows = ["..........", ".┌─R─┐....", ".B...├─T..", ".│...S....", ".└───┘....", ".........."];
        solve(job, &rows, &[('B', bat, Dir::N), ('R', top, Dir::W), ('S', bottom, Dir::N), ('T', TP, Dir::W)])
    }

    #[test]
    fn boat_gauge_wants_a_third_from_e12_values() {
        assert_eq!(divider(6, BAT12, R(4700.0), R(2200.0)), [true, true]);
        assert_eq!(divider(6, BAT12, R(1000.0), R(470.0)), [true, true]);
        // 2.2k over 1k is 0.3125: just under on both batteries.
        assert_eq!(divider(6, BAT12, R(2200.0), R(1000.0)), [false, false]);
        // The small pair works but drains the battery: no battery tick.
        let job = Job::load(6);
        let rows = ["..........", ".┌─R─┐....", ".B...├─T..", ".│...S....", ".└───┘....", ".........."];
        let rows: Vec<String> = rows.iter().map(|r| r.replace('.', " ")).collect();
        let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
        let small = crate::circuit::Board::parse(&refs, &[('B', BAT12, Dir::N), ('R', R(1000.0), Dir::W), ('S', R(470.0), Dir::N), ('T', TP, Dir::W)]).unwrap();
        assert!(!job.ticks(true, job.score(&small)).frugal);
    }

    #[test]
    fn night_light_wants_a_10k_pot_and_a_fixed_resistor() {
        let rows = [".┌─┐......", ".│.P─R──┐.", ".B......L.", ".└──────┘.", "..........", ".........."];
        let red = Kind::Led(crate::circuit::Colour::Red);
        let with = |pot: f64, r: Kind, facing: Dir| solve(7, &rows, &[('B', BAT45, Dir::N), ('L', red, Dir::N), ('P', Kind::Pot { ohms: pot, turn: 0.5 }, facing), ('R', r, Dir::W)]);
        assert_eq!(with(10e3, R(220.0), Dir::W), [true, true]);
        assert_eq!(with(10e3, R(150.0), Dir::W), [true, true]);
        // 100 ohm: too bright turned up. The 1k pot can't turn it down far enough.
        assert_eq!(with(10e3, R(100.0), Dir::W), [false, true]);
        assert_eq!(with(1e3, R(220.0), Dir::W), [true, false]);
        // No fixed resistor: a wire where R was burns the LED turned up.
        let bare = [".┌─┐......", ".│.P────┐.", ".B......L.", ".└──────┘.", "..........", ".........."];
        assert_eq!(solve(7, &bare, &[('B', BAT45, Dir::N), ('L', red, Dir::N), ('P', Kind::Pot { ohms: 10e3, turn: 0.5 }, Dir::W)]), [false, true]);
    }

    #[test]
    fn fuel_gauge_wants_15k_above_the_pot_as_a_divider() {
        let rows = ["..........", ".┌─R─┐....", ".B...P─T..", ".│...│....", ".└───┘....", ".........."];
        let pot = Kind::Pot { ohms: 10e3, turn: 0.0 };
        let with = |top: f64| solve(8, &rows, &[('B', BAT12, Dir::N), ('R', R(top), Dir::W), ('P', pot, Dir::N), ('T', TP, Dir::W)]);
        assert_eq!(with(15e3), [true, true, true]);
        // 10k: 6 V full, off the gauge. 22k: 3.75 V full, the needle never gets there.
        assert_eq!(with(10e3)[..2], [false, false]);
        assert_eq!(with(22e3)[..2], [false, false]);
        // The pot as a plain variable resistor, its far end left free: the reading never moves.
        let rheostat = ["..........", ".┌─R─┐....", ".B...P─T..", ".│........", ".└────────", ".........."];
        let r = solve(8, &rheostat, &[('B', BAT12, Dir::N), ('R', R(15e3), Dir::W), ('P', pot, Dir::N), ('T', TP, Dir::W)]);
        assert!(r.iter().any(|ok| !ok), "{r:?}");
    }

    #[test]
    fn light_meter_wants_the_ldr_on_top_and_a_resistor_near_its_middle() {
        let ldr = Kind::Ldr { lux: 100.0 };
        assert_eq!(divider(9, BAT45, ldr, R(4700.0)), [true, true]);
        assert_eq!(divider(9, BAT45, ldr, R(2200.0)), [true, true]);
        assert_eq!(divider(9, BAT45, ldr, R(1000.0)), [false, true]);
        assert_eq!(divider(9, BAT45, ldr, R(10e3)), [true, false]);
        // Upside down: dark reads high.
        assert_eq!(divider(9, BAT45, R(4700.0), ldr), [false, false]);
    }

    #[test]
    fn fridge_gauge_wants_the_thermistor_on_top_over_22k() {
        let ntc = Kind::Ntc { celsius: 20.0 };
        assert_eq!(divider(10, BAT45, ntc, R(22e3)), [true, true]);
        assert_eq!(divider(10, BAT45, ntc, R(10e3))[0], false);
        assert_eq!(divider(10, BAT45, ntc, R(47e3))[0], false);
        assert_eq!(divider(10, BAT45, R(22e3), ntc), [false, false]);
    }

    #[test]
    fn trailer_light_wants_a_2_amp_fuse() {
        let rows = ["..........", ".┌─F─┬──┐.", ".B...K..X.", ".│...Z..│.", ".└───┴──┘.", ".........."];
        let with = |amps: f64| {
            solve(
                11,
                &rows,
                &[
                    ('B', BAT12, Dir::N),
                    ('F', Kind::Fuse { amps }, Dir::W),
                    ('K', BUTTON, Dir::N),
                    ('Z', Kind::Resistor { ohms: 2.7, watts: 100.0 }, Dir::N),
                    ('X', Kind::Lamp { ohms: 8.0, watts: 18.0 }, Dir::N),
                ],
            )
        };
        assert_eq!(with(2.0), [true, true]);
        // 500 mA blows with the lamp alone; 5 A sits through the fault.
        assert_eq!(with(0.5), [false, true]);
        assert_eq!(with(5.0), [true, false]);
        // No fuse at all: a wire where F was. The fault isn't cut off.
        let bare = ["..........", ".┌───┬──┐.", ".B...K..X.", ".│...Z..│.", ".└───┴──┘.", ".........."];
        let r = solve(11, &bare, &[('B', BAT12, Dir::N), ('K', BUTTON, Dir::N), ('Z', Kind::Resistor { ohms: 2.7, watts: 100.0 }, Dir::N), ('X', Kind::Lamp { ohms: 8.0, watts: 18.0 }, Dir::N)]);
        assert_eq!(r, [true, false]);
    }

    #[test]
    fn porch_lamp_wants_a_button() {
        let rows = ["..........", ".┌──P───┐.", ".B......X.", ".└──────┘."];
        let button = [('B', BAT45, Dir::N), ('X', LAMP, Dir::N), ('P', BUTTON, Dir::W)];
        assert_eq!(solve(1, &rows, &button), [true, true]);
        // A switch stays on when you let go: and there's no PB1 for the bench to press.
        let switch = [('B', BAT45, Dir::N), ('X', LAMP, Dir::N), ('P', Kind::Switch { closed: true }, Dir::W)];
        assert_eq!(solve(1, &rows, &switch), [false, false]);
    }

    #[test]
    fn two_doors_want_buttons_side_by_side() {
        let legend = [('B', BAT45, Dir::N), ('X', LAMP, Dir::N), ('P', BUTTON, Dir::W), ('Q', BUTTON, Dir::W)];
        let parallel = ["..........", ".┌─┬P┬──┐.", ".B.└Q┘..X.", ".└──────┘."];
        assert_eq!(solve(2, &parallel, &legend), [true, true, true]);
        let series = ["..........", ".┌─P─Q──┐.", ".B......X.", ".└──────┘."];
        assert_eq!(solve(2, &series, &legend), [false, false, true]);
    }

    #[test]
    fn dolls_house_wants_a_resistor_per_led() {
        let legend = |r: f64| {
            [
                ('B', BAT45, Dir::N),
                ('L', Kind::Led(crate::circuit::Colour::Red), Dir::W),
                ('M', Kind::Led(crate::circuit::Colour::Green), Dir::W),
                ('R', R(r), Dir::W),
                ('S', R(r), Dir::W),
            ]
        };
        let own = ["..........", ".┌─R─L──┐.", ".├─S─M──┤.", ".B......│.", ".└──────┘."];
        assert_eq!(solve(3, &own, &legend(150.0)), [true, true]);
        assert_eq!(solve(3, &own, &legend(220.0)), [true, true]);
        // One resistor for both: the red LED, with the lower forward voltage, takes most of the
        // current (18.7 mA against 8.0 mA fresh), and on tired cells the green one starves.
        let shared = ["..........", ".┌───L──┐.", ".├───M──┤.", ".B......│.", ".└──R───┘."];
        assert_eq!(solve(3, &shared, &legend(100.0)), [true, false]);
    }

    #[test]
    fn christmas_star_runs_three_leds_in_series() {
        let legend = |r: f64| {
            let red = Kind::Led(crate::circuit::Colour::Red);
            [
                ('B', Kind::Battery { volts: 9.0, r_int: 1.5 }, Dir::N),
                ('A', red, Dir::W),
                ('C', red, Dir::W),
                ('E', red, Dir::W),
                ('R', R(r), Dir::W),
                ('S', Kind::Switch { closed: true }, Dir::W),
            ]
        };
        let rows = ["..........", ".┌─A─C─E┐.", ".│......│.", ".B......│.", ".└─R──S─┘."];
        assert_eq!(solve(4, &rows, &legend(220.0)), [true, true, true]);
        assert_eq!(solve(4, &rows, &legend(100.0)), [false, true, true]);
        assert_eq!(solve(4, &rows, &legend(470.0)), [true, false, true]);
    }

    #[test]
    fn cafe_sign_needs_a_resistor_that_can_take_the_heat() {
        let legend = |watts: f64, ohms: f64| {
            [
                ('B', Kind::Battery { volts: 12.0, r_int: 0.05 }, Dir::N),
                ('X', Kind::Lamp { ohms: 20.0, watts: 1.8 }, Dir::N),
                ('R', Kind::Resistor { ohms, watts }, Dir::W),
            ]
        };
        let rows = ["..........", ".┌──R───┐.", ".B......X.", ".└──────┘."];
        assert_eq!(solve(5, &rows, &legend(2.0, 22.0)), [true, true]);
        assert_eq!(solve(5, &rows, &legend(0.25, 22.0)), [false, false]);
        assert_eq!(solve(5, &rows, &legend(0.25, 47.0)), [false, false]);
        // The 2 W resistor works but costs more than the master's four 1 W ones side by side.
        let job = Job::load(5);
        let mut b = job.board.clone();
        b.place(Kind::Resistor { ohms: 22.0, watts: 2.0 }, Pos { x: 4, y: 1 }, Dir::W, false);
        let mine = job.score(&b);
        assert!((mine.cost - 1.50).abs() < 1e-9 && (job.par().cost - 1.20).abs() < 1e-9);
        assert_eq!(job.ticks(true, mine), Ticks { works: true, thrifty: false, frugal: true });
        // And the master's four share the heat: each well inside its 1 W at 13.8 V.
        let case = job.cases.iter().find(|c| c.battery == Some(13.8)).unwrap();
        assert!(run_case(&job.master, case).pass);
        let mut hot = job.master.clone();
        for p in &mut hot.parts {
            if let Kind::Battery { volts, .. } = &mut p.kind {
                *volts = 13.8;
            }
        }
        let mut live = crate::circuit::Live::build(&hot);
        live.sim.solve_dc().unwrap();
        for label in ["R1", "R2", "R3", "R4"] {
            let w = live.reading(&hot, hot.part(label).unwrap()).watts;
            assert!(w > 0.5 && w < 0.7, "{label} {w}");
        }
    }

    #[test]
    fn bike_lamp_wants_150_or_220_ohms() {
        assert_eq!(bike_lamp_with(100.0), [false, true, true]);
        assert_eq!(bike_lamp_with(150.0), [true, true, true]);
        assert_eq!(bike_lamp_with(220.0), [true, true, true]);
        assert_eq!(bike_lamp_with(1000.0), [false, false, true]);
    }

    fn cap(farads: f64, volts: f64) -> Kind {
        Kind::Capacitor { farads, volts }
    }
    const RED: Kind = Kind::Led(crate::circuit::Colour::Red);
    const BAT9: Kind = Kind::Battery { volts: 9.0, r_int: 1.5 };
    const SW: Kind = Kind::Switch { closed: true };

    #[test]
    fn standlight_wants_the_biggest_capacitor_across_the_supply() {
        let rows = ["..........", ".┌─S┬─R─┐.", ".B..C...L.", ".└──┴───┘.", "..........", ".........."];
        let with = |c: Kind, facing: Dir| solve(12, &rows, &[('B', BAT45, Dir::N), ('S', SW, Dir::W), ('R', R(220.0), Dir::W), ('C', c, facing), ('L', RED, Dir::N)]);
        assert_eq!(with(cap(4700e-6, 16.0), Dir::N), [true, true]);
        assert_eq!(with(cap(2200e-6, 16.0), Dir::N), [true, false]);
        // Backwards it vents.
        assert_eq!(with(cap(4700e-6, 16.0), Dir::S)[1], false);
    }

    #[test]
    fn courtesy_light_wants_1k_and_2200uf_rated_16v_before_the_resistor() {
        let rows = ["..........", ".┌─K┬─R─┐.", ".B..C...L.", ".└──┴───┘.", "..........", ".........."];
        let with = |r: f64, c: Kind| solve(13, &rows, &[('B', BAT12, Dir::N), ('K', BUTTON, Dir::W), ('R', R(r), Dir::W), ('C', c, Dir::N), ('L', RED, Dir::N)]);
        assert_eq!(with(1000.0, cap(2200e-6, 16.0)), [true, true]);
        assert_eq!(with(470.0, cap(2200e-6, 16.0))[0], false, "too bright");
        assert_eq!(with(2200.0, cap(2200e-6, 16.0))[0], false, "too dim");
        assert_eq!(with(1000.0, cap(1000e-6, 16.0))[1], false, "too short");
        assert_eq!(with(1000.0, cap(4700e-6, 16.0))[1], false, "too long");
        assert_eq!(with(1000.0, cap(2200e-6, 10.0))[1], false, "the 10 V can vents on 12 V");
        // Across the LED instead, the capacitor empties through it in a blink.
        let across = ["..........", ".┌─K─R┬─┐.", ".B....C.L.", ".└────┴─┘.", "..........", ".........."];
        let r = solve(13, &across, &[('B', BAT12, Dir::N), ('K', BUTTON, Dir::W), ('R', R(1000.0), Dir::W), ('C', cap(2200e-6, 16.0), Dir::N), ('L', RED, Dir::N)]);
        assert_eq!(r[1], false);
    }

    #[test]
    fn darkroom_sign_wants_two_small_capacitors_side_by_side() {
        let one = ["..........", ".┌─S┬─R─┐.", ".B..C...L.", ".└──┴───┘.", "..........", ".........."];
        let two = ["..........", ".┌─S┬┬R─┐.", ".B..CD..L.", ".└──┴┴──┘.", "..........", ".........."];
        let base = |c: Kind, d: Kind| vec![('B', BAT9, Dir::N), ('S', SW, Dir::W), ('R', R(1000.0), Dir::W), ('C', c, Dir::N), ('D', d, Dir::N), ('L', RED, Dir::N)];
        assert_eq!(solve(14, &two, &base(cap(220e-6, 16.0), cap(220e-6, 16.0))), [true, true, true]);
        assert_eq!(solve(14, &one, &base(cap(220e-6, 16.0), cap(220e-6, 16.0))), [true, false, true], "one can't ride the bump");
        assert_eq!(solve(14, &one, &base(cap(1000e-6, 16.0), cap(220e-6, 16.0))), [true, true, false], "1000 uF lingers");
    }

    #[test]
    fn level_crossing_waits_about_1_1_r_c() {
        let rows = ["..........", ".┌─S─R─┬─T", ".B.....C..", ".└─────┘..", "..........", ".........."];
        let with = |r: f64, f: f64| solve(15, &rows, &[('B', BAT45, Dir::N), ('S', SW, Dir::W), ('R', R(r), Dir::W), ('C', cap(f, 16.0), Dir::N), ('T', TP, Dir::W)]);
        assert_eq!(with(10e3, 100e-6), [true, true]);
        assert_eq!(with(2200.0, 470e-6), [true, true]);
        assert_eq!(with(4700.0, 100e-6), [true, false], "too quick");
        assert_eq!(with(22e3, 100e-6), [true, false], "too slow");
    }

    #[test]
    fn crossing_lifts_wants_a_33k_bleeder() {
        let rows = ["..........", ".┌─S─R─┬┬T", ".B.....CQ.", ".└─────┴┘.", "..........", ".........."];
        let with = |rb: f64| solve(16, &rows, &[('B', BAT45, Dir::N), ('S', SW, Dir::W), ('R', R(10e3), Dir::W), ('C', cap(100e-6, 16.0), Dir::N), ('Q', R(rb), Dir::N), ('T', TP, Dir::W)]);
        assert_eq!(with(33e3), [true, true]);
        assert_eq!(with(22e3), [false, true], "too slow down");
        assert_eq!(with(47e3), [true, false], "too slow up");
        assert_eq!(with(10e3), [false, true], "never gets to 3 V, so it drops quickly");
        let none = ["..........", ".┌─S─R─┬─T", ".B.....C..", ".└─────┘..", "..........", ".........."];
        let r = solve(16, &none, &[('B', BAT45, Dir::N), ('S', SW, Dir::W), ('R', R(10e3), Dir::W), ('C', cap(100e-6, 16.0), Dir::N), ('T', TP, Dir::W)]);
        assert_eq!(r, [true, false], "never lifts");
    }

    #[test]
    fn puppet_theatre_fades_both_ways() {
        let rows = ["..........", ".┌S─A┬─R┐.", ".B...C..L.", ".└───┴──┘.", "..........", ".........."];
        let with = |r1: f64, f: f64, r2: f64| solve(17, &rows, &[('B', BAT9, Dir::N), ('S', SW, Dir::W), ('A', R(r1), Dir::W), ('C', cap(f, 16.0), Dir::N), ('R', R(r2), Dir::W), ('L', RED, Dir::N)]);
        assert_eq!(with(470.0, 2200e-6, 470.0), [true, true, true]);
        assert_eq!(with(1000.0, 4700e-6, 220.0), [true, true, true]);
        assert_eq!(with(1000.0, 2200e-6, 470.0)[1], false, "never bright enough");
        assert_eq!(with(220.0, 1000e-6, 470.0)[0], false, "up too fast");
        // No capacitor: on at once, off at once.
        let bare = ["..........", ".┌S─A──R┐.", ".B......L.", ".└──────┘.", "..........", ".........."];
        let r = solve(17, &bare, &[('B', BAT9, Dir::N), ('S', SW, Dir::W), ('A', R(470.0), Dir::W), ('R', R(470.0), Dir::W), ('L', RED, Dir::N)]);
        assert_eq!((r[0], r[2]), (false, false));
    }

    const RELAY12: Kind = Kind::Relay { volts: 12.0, nc: false };
    const RELAY12NC: Kind = Kind::Relay { volts: 12.0, nc: true };

    #[test]
    fn spotlamps_want_a_12v_relay_between_the_thin_wire_and_the_lamp() {
        let rows = ["..........", ".┌─S─W─┐..", ".├─────K┐.", ".B.....│X.", ".└─────┴┘.", ".........."];
        let spot = Kind::Lamp { ohms: 2.62, watts: 55.0 };
        let with = |k: Kind| solve(18, &rows, &[('B', BAT12, Dir::N), ('S', SW, Dir::W), ('W', Kind::Resistor { ohms: 1.5, watts: 2.0 }, Dir::W), ('K', k, Dir::N), ('X', spot, Dir::N)]);
        assert_eq!(with(RELAY12), [true, true]);
        assert_eq!(with(RELAY12NC), [false, false], "lit the wrong way round");
        assert_eq!(with(Kind::Relay { volts: 5.0, nc: false })[0], false, "the 5 V coil burns on 12 V");
        // Straight through the thin wire: dim, and the wire burns.
        let direct = ["..........", ".┌─S─W─┐..", ".│.....└┐.", ".B......X.", ".└──────┘.", ".........."];
        let r = solve(18, &direct, &[('B', BAT12, Dir::N), ('S', SW, Dir::W), ('W', Kind::Resistor { ohms: 1.5, watts: 2.0 }, Dir::W), ('X', spot, Dir::N)]);
        assert_eq!(r[0], false);
    }

    #[test]
    fn coffee_roaster_wants_a_relay_holding_itself_in_across_start() {
        let rows = ["..........", ".┌─┬Q┬P─┐.", ".L.├┐│..│.", ".│.└K┘..B.", ".└──┴───┘.", ".........."];
        let lamp = Kind::Lamp { ohms: 28.8, watts: 5.0 };
        let stop = Kind::Button { held: false, nc: true };
        let with = |rows: &[&str], k: Kind| solve(19, rows, &[('B', BAT12, Dir::N), ('Q', BUTTON, Dir::W), ('P', stop, Dir::W), ('L', lamp, Dir::N), ('K', k, Dir::N)]);
        assert_eq!(with(&rows, RELAY12), [true, true, true]);
        assert_ne!(with(&rows, RELAY12NC), [true, true, true], "an NC relay buzzes on its own contact");
        // No holding contact: it stops when START is let go.
        let plain = ["..........", ".┌─┬Q┬P─┐.", ".L.├┐...│.", ".│..K...B.", ".└──┴───┘.", ".........."];
        assert_eq!(with(&plain, RELAY12), [true, false, true]);
    }

    #[test]
    fn railway_signal_wants_green_through_an_nc_relay() {
        let rows = ["..........", ".┌─S┬─R─┐.", ".├──K─Q┐D.", ".B..│..G│.", ".└──┴──┴┘.", ".........."];
        let bat9 = Kind::Battery { volts: 9.0, r_int: 1.5 };
        let green = Kind::Led(Colour::Green);
        let with = |k: Kind| solve(20, &rows, &[('B', bat9, Dir::N), ('S', SW, Dir::W), ('R', R(470.0), Dir::W), ('Q', R(470.0), Dir::W), ('K', k, Dir::N), ('D', RED, Dir::N), ('G', green, Dir::N)]);
        assert_eq!(with(Kind::Relay { volts: 9.0, nc: true }), [true, true, true, true]);
        assert_eq!(with(Kind::Relay { volts: 9.0, nc: false }), [true, false, false, true], "NO gets green backwards");
    }

    #[test]
    fn doorbell_buzzer_wants_a_5v_relay_through_its_own_nc_contact() {
        let rows = ["..........", ".┌─P┐┌┐...", ".│..└K┘...", ".B...│....", ".└───┘....", ".........."];
        let with = |rows: &[&str], k: Kind| solve(21, rows, &[('B', BAT45, Dir::N), ('P', BUTTON, Dir::W), ('K', k, Dir::N)]);
        assert_eq!(with(&rows, Kind::Relay { volts: 5.0, nc: true }), [true]);
        assert_eq!(with(&rows, Kind::Relay { volts: 5.0, nc: false }), [false], "NO never starts");
        assert_eq!(with(&rows, RELAY12NC), [false], "12 V can't pull in on 4.5 V");
        // The coil straight from the button: it pulls in and stays.
        let straight = ["..........", ".┌─P─┐....", ".│...K....", ".B...│....", ".└───┘....", ".........."];
        assert_eq!(with(&straight, Kind::Relay { volts: 5.0, nc: true }), [false]);
    }

    #[test]
    fn cellar_light_wants_4700uf_16v_across_the_coil() {
        let rows = [".┌───┐....", ".├─P┐│....", ".│..├K┐...", ".B..CX│...", ".└──┴┴┘...", ".........."];
        let lamp = Kind::Lamp { ohms: 28.8, watts: 5.0 };
        let with = |c: Kind| solve(22, &rows, &[('B', BAT12, Dir::N), ('P', BUTTON, Dir::W), ('K', Kind::Relay { volts: 12.0, nc: false }, Dir::W), ('C', c, Dir::N), ('X', lamp, Dir::N)]);
        assert_eq!(with(cap(4700e-6, 16.0)), [true, true]);
        assert_eq!(with(cap(2200e-6, 16.0)), [true, false], "too short");
        assert_eq!(with(cap(4700e-6, 10.0))[0], false, "the 10 V can vents");
    }

    #[test]
    fn lighthouse_flashes_every_2_5_s_on_2200uf() {
        let rows = ["..........", ".┌─X┐┌┐...", ".│..└K┤...", ".B...│C...", ".└───┴┘...", ".........."];
        let lamp = Kind::Lamp { ohms: 28.8, watts: 5.0 };
        let with = |k: Kind, c: f64| solve(23, &rows, &[('B', BAT12, Dir::N), ('X', lamp, Dir::W), ('K', k, Dir::N), ('C', cap(c, 16.0), Dir::N)]);
        assert_eq!(with(RELAY12NC, 2200e-6), [true]);
        assert_eq!(with(RELAY12NC, 1000e-6), [false], "too quick");
        assert_eq!(with(RELAY12NC, 4700e-6), [false], "too slow");
        assert_eq!(with(RELAY12, 2200e-6), [false], "NO never starts");
    }

    #[test]
    fn flash_battery_wants_a_diode_in_series_the_right_way_round() {
        let rows = ["..........", ".┌─D┬─R─┐.", ".B..C...L.", ".└──┴───┘.", "..........", ".........."];
        let bat9 = Kind::Battery { volts: 9.0, r_int: 1.5 };
        let green = Kind::Led(Colour::Green);
        let with = |rows: &[&str], d: Dir| solve(24, rows, &[('B', bat9, Dir::N), ('D', Kind::Diode, d), ('C', cap(1000e-6, 16.0), Dir::N), ('R', R(470.0), Dir::W), ('L', green, Dir::N)]);
        assert_eq!(with(&rows, Dir::W), [true, true]);
        assert_eq!(with(&rows, Dir::E), [false, false], "backwards: dark, and it passes a reversed battery");
        let bare = ["..........", ".┌──┬─R─┐.", ".B..C...L.", ".└──┴───┘.", "..........", ".........."];
        assert_eq!(with(&bare, Dir::W), [true, false], "no diode: the capacitor vents");
    }

    #[test]
    fn which_door_wants_a_diode_from_each_door_to_the_bell() {
        let rows = [".┌─┬───┐..", ".│.P...Q..", ".B.├D┬E┤..", ".│.L.X.M..", ".│.R.│.S..", ".└─┴─┴─┘.."];
        let lamp = Kind::Lamp { ohms: 15.0, watts: 1.35 };
        let legend = |rows: &[&str]| {
            solve(
                25,
                rows,
                &[
                    ('B', BAT45, Dir::N),
                    ('P', BUTTON, Dir::N),
                    ('Q', BUTTON, Dir::N),
                    ('L', RED, Dir::N),
                    ('M', Kind::Led(Colour::Green), Dir::N),
                    ('X', lamp, Dir::N),
                    ('R', R(150.0), Dir::N),
                    ('S', R(150.0), Dir::N),
                    ('D', Kind::Diode, Dir::W),
                    ('E', Kind::Diode, Dir::E),
                ],
            )
        };
        assert_eq!(legend(&rows), [true; 6]);
        let joined = [".┌─┬───┐..", ".│.P...Q..", ".B.├─┬─┤..", ".│.L.X.M..", ".│.R.│.S..", ".└─┴─┴─┘.."];
        assert_eq!(legend(&joined), [true, false, true, true, false, true], "plain copper lights both");
    }

    #[test]
    fn flood_alarm_wants_a_transistor_and_470_ohm() {
        let rows = ["..........", ".┌─S─W─┐..", ".├─R─L─Q┐.", ".B......│.", ".└──────┘.", ".........."];
        let with = |r: f64| solve(26, &rows, &[('B', BAT9, Dir::N), ('S', SW, Dir::W), ('W', R(10e3), Dir::W), ('R', R(r), Dir::W), ('L', RED, Dir::W), ('Q', Kind::Npn { flip: false }, Dir::W)]);
        assert_eq!(with(470.0), [true, true]);
        assert_eq!(with(220.0), [true, false], "too bright");
        assert_eq!(with(1000.0), [true, false], "too dim");
    }

    #[test]
    fn oil_warning_wants_a_transistor_not_with_10k_and_680() {
        let rows = [".┌──┬───┐.", ".C..R...│.", ".L..│...B.", ".│..│...│.", ".Q──┴S┐.│.", ".└────┴─┘."];
        let with = |rc: f64, rb: f64| solve(27, &rows, &[('B', BAT12, Dir::N), ('S', SW, Dir::W), ('L', RED, Dir::N), ('C', R(rc), Dir::N), ('R', R(rb), Dir::N), ('Q', Kind::Npn { flip: false }, Dir::N)]);
        assert_eq!(with(680.0, 10e3), [true, true]);
        assert_eq!(with(470.0, 10e3), [true, false], "too bright");
        assert_eq!(with(1000.0, 10e3), [true, false], "too dim");
        assert_eq!(with(680.0, 1e6), [true, false], "1M can't switch it on");
        assert_eq!(with(680.0, 100.0)[0], false, "100 ohm burns while the switch shorts it");
    }

    #[test]
    fn oil_warning_also_passes_with_the_switch_shorting_the_led() {
        // No transistor: the LED and its resistor, the switch across the LED. Cheaper than the
        // master's, and it wastes 17 mA while the engine runs; honest, so it passes.
        let rows = [".┌R─────┐.", ".├──┐...│.", ".L..│...B.", ".│..│...│.", ".│..└S┐.│.", ".└────┴─┘."];
        let r = solve(27, &rows, &[('B', BAT12, Dir::N), ('S', SW, Dir::W), ('L', RED, Dir::N), ('R', R(680.0), Dir::W)]);
        assert_eq!(r, [true, true]);
    }

    /// The sensor-and-transistor layout of the freezer alarm and the jetty light: `top` above
    /// the base, `bottom` below it.
    fn sensor_switch(job: usize, top: Kind, bottom: Kind, led: Kind) -> Vec<bool> {
        let rows = [".┌─┬────┐.", ".C.T....│.", ".L.│....B.", ".Q─┤....│.", ".│.U....│.", ".└─┴────┘."];
        solve(job, &rows, &[('B', BAT9, Dir::N), ('C', R(470.0), Dir::N), ('L', led, Dir::N), ('Q', Kind::Npn { flip: false }, Dir::N), ('T', top, Dir::N), ('U', bottom, Dir::N)])
    }

    #[test]
    fn freezer_alarm_wants_the_thermistor_over_3k3() {
        let ntc = Kind::Ntc { celsius: -18.0 };
        assert_eq!(sensor_switch(28, ntc, R(3300.0), RED), [true, true, true]);
        assert_eq!(sensor_switch(28, ntc, R(2200.0), RED), [true, false, true], "too dim at 4 C");
        assert_eq!(sensor_switch(28, ntc, R(10e3), RED)[0], false, "lit in the freezer");
    }

    #[test]
    fn jetty_light_wants_47k_over_the_ldr() {
        let ldr = Kind::Ldr { lux: 1000.0 };
        let yellow = Kind::Led(Colour::Yellow);
        let flip = |top: Kind, bottom: Kind| {
            let rows = [".┌─┬────┐.", ".C.U....│.", ".L.│....B.", ".Q─┤....│.", ".│.T....│.", ".└─┴────┘."];
            solve(29, &rows, &[('B', BAT9, Dir::N), ('C', R(470.0), Dir::N), ('L', yellow, Dir::N), ('Q', Kind::Npn { flip: false }, Dir::N), ('T', bottom, Dir::N), ('U', top, Dir::N)])
        };
        assert_eq!(flip(R(47e3), ldr), [true, true, true, true]);
        assert_eq!(flip(R(10e3), ldr)[1], false, "lit when overcast");
        assert_eq!(flip(R(1e6), ldr)[3], false, "never bright");
    }

    #[test]
    fn occupied_sign_wants_a_flip_flop_with_10k_across() {
        let rows = ["┌────────┐", "│.┌Y──┐..│", "└Z┤┌─Wx┬V┤", "..Q┤..├q.│", "..LP..OM.B", "..└┴──┴┴─┘"];
        let with = |rb: f64, flip: bool| {
            solve(
                30,
                &rows,
                &[
                    ('B', BAT9, Dir::N),
                    ('L', RED, Dir::N),
                    ('M', Kind::Led(Colour::Green), Dir::N),
                    ('P', BUTTON, Dir::N),
                    ('O', BUTTON, Dir::N),
                    ('Y', R(rb), Dir::W),
                    ('W', R(rb), Dir::W),
                    ('Z', R(470.0), Dir::W),
                    ('V', R(470.0), Dir::W),
                    ('Q', Kind::Npn { flip: false }, Dir::N),
                    ('q', Kind::Npn { flip }, Dir::N),
                ],
            )
        };
        assert_eq!(with(10e3, true), [true; 4]);
        assert_ne!(with(1e6, true), [true; 4], "1M can't switch a side fully on");
        assert_ne!(with(10e3, false), [true; 4], "Q2's base faces away");
    }

    #[test]
    fn crossing_lights_want_82k_and_10uf_in_a_blinker() {
        let rows = [".┌─┬─┬─┬─┐", ".R.Y.W.V.│", ".L.│.│.M.│", ".├Cx─┤.│.B", ".│.├─xD┤.│", ".Q─┘.└─q.│", ".└─────┴─┘"];
        let with = |rb: f64, c: f64| {
            solve(
                31,
                &rows,
                &[
                    ('B', BAT9, Dir::N),
                    ('L', RED, Dir::N),
                    ('M', RED, Dir::N),
                    ('R', R(470.0), Dir::N),
                    ('V', R(470.0), Dir::N),
                    ('Y', R(rb), Dir::N),
                    ('W', R(rb), Dir::N),
                    ('C', cap(c, 16.0), Dir::W),
                    ('D', cap(c, 16.0), Dir::E),
                    ('Q', Kind::Npn { flip: false }, Dir::N),
                    ('q', Kind::Npn { flip: true }, Dir::N),
                ],
            )
        };
        assert_eq!(with(82e3, 10e-6), [true; 2]);
        assert_eq!(with(68e3, 10e-6), [false; 2], "too fast");
        assert_eq!(with(100e3, 10e-6), [false; 2], "too slow");
        assert_eq!(with(82e3, 100e-6), [false; 2], "far too slow");
    }

    #[test]
    fn quiz_timer_wants_a_one_shot_of_82k_and_100uf() {
        let rows = [".┌─┬─┬─┬─┐", ".R.T.P.V.│", ".L.│.S.M.│", ".├Kx─┤.│.B", ".│.├─xC┤.│", ".Q─┘.└─q.│", ".└─────┴─┘"];
        let with = |rows: &[&str], rt: f64, c: f64| {
            solve(
                32,
                rows,
                &[
                    ('B', BAT9, Dir::N),
                    ('L', RED, Dir::N),
                    ('M', Kind::Led(Colour::Green), Dir::N),
                    ('P', BUTTON, Dir::N),
                    ('R', R(470.0), Dir::N),
                    ('V', R(470.0), Dir::N),
                    ('T', R(rt), Dir::N),
                    ('S', R(10e3), Dir::N),
                    ('K', R(47e3), Dir::W),
                    ('C', cap(c, 16.0), Dir::E),
                    ('Q', Kind::Npn { flip: false }, Dir::N),
                    ('q', Kind::Npn { flip: true }, Dir::N),
                ],
            )
        };
        assert_eq!(with(&rows, 82e3, 100e-6), [true; 5]);
        assert_eq!(with(&rows, 68e3, 100e-6)[2], false, "too short");
        assert_eq!(with(&rows, 100e3, 100e-6)[2], false, "too long");
        assert_eq!(with(&rows, 82e3, 47e-6)[2], false, "far too short");
        assert_eq!(with(&rows, 82e3, 220e-6)[2], false, "far too long");
        let bare = [".┌─┬─┬─┬─┐", ".R.T.P.V.│", ".L.│.│.M.│", ".├Kx─┤.│.B", ".│.├─xC┤.│", ".Q─┘.└─q.│", ".└─────┴─┘"];
        assert_ne!(with(&bare, 82e3, 100e-6), [true; 5], "the button straight to the base");
    }

    #[test]
    fn boathouse_alarm_wants_a_nand_as_not_with_a_big_pull_up() {
        let master = [
            "┌┬──────┬┐",
            "P│......││",
            "│IIIIIII││",
            "│IIIIIII│B",
            "└┴┤└─RLx┘│",
            "..S....│.│",
            "..└────┴─┘",
        ];
        let legend = |pull: f64| {
            vec![
                ('B', BAT9, Dir::N),
                ('S', Kind::Switch { closed: true }, Dir::N),
                ('L', RED, Dir::E),
                ('P', R(pull), Dir::N),
                ('R', R(470.0), Dir::W),
                ('I', Kind::Chip(Chip::Cd4011), Dir::W),
            ]
        };
        assert_eq!(solve(33, &master, &legend(1e6)), [true; 3]);
        // No pull-up: the inputs float, keep their last level, and the lamp never lights.
        let mut bare = master;
        bare[1] = ".│......││";
        bare[2] = ".IIIIIII││";
        bare[3] = ".IIIIIII│B";
        bare[4] = ".┴┤└─RLx┘│";
        bare[0] = ".┌──────┬┐";
        assert_eq!(solve(33, &bare, &legend(1e6)), [true, false, true]);
        // Smaller pull-ups work but drain the battery.
        let job = Job::load(33);
        let board = |pull: f64| {
            let rows: Vec<String> = master.iter().map(|r| r.replace('.', " ")).collect();
            let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
            let mut b = crate::circuit::Board::parse(&refs, &legend(pull)).unwrap();
            for p in &mut b.parts {
                p.fixed = matches!(p.label.as_str(), "BT1" | "SW1" | "LED1");
            }
            b
        };
        let par = job.par().hours.unwrap();
        assert!(par > 50_000.0, "{par} h");
        for pull in [10e3, 100e3] {
            assert_eq!(solve(33, &master, &legend(pull)), [true; 3]);
            assert!(job.score(&board(pull)).hours.unwrap() < par / 5.0, "{pull}");
        }
        // No chip: the LED and its resistor across the door contact. It works, all season long.
        let hack = ["........┌┐", "........│R", "........││", "........│B", "..┌┐.┌L┬┘│", "..S└─x─┘.│", "..└──┴───┘"];
        let hack_legend = [('B', BAT9, Dir::N), ('S', Kind::Switch { closed: true }, Dir::N), ('L', RED, Dir::E), ('R', R(470.0), Dir::N)];
        assert_eq!(solve(33, &hack, &hack_legend), [true; 3]);
        let rows: Vec<String> = hack.iter().map(|r| r.replace('.', " ")).collect();
        let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
        let mut b = crate::circuit::Board::parse(&refs, &hack_legend).unwrap();
        for p in &mut b.parts {
            p.fixed = p.label != "R1";
        }
        let hours = job.score(&b).hours.unwrap();
        assert!(hours < 50.0, "{hours} h");
    }

    #[test]
    fn a_flip_flop_remembers_across_a_rebuild() {
        use crate::circuit::Live;
        let mut b = Job::load(30).master;
        let mut live = Live::build(&b);
        let run = |live: &mut Live| {
            for _ in 0..200 {
                live.sim.step(0.001).unwrap();
            }
        };
        let press = |b: &mut crate::circuit::Board, live: &mut Live, label: &str, held: bool| {
            let i = b.part(label).unwrap();
            b.parts[i].kind = Kind::Button { held, nc: false };
            live.set_switch(i, held);
        };
        run(&mut live);
        for (button, lit, dark) in [("PB2", "LED1", "LED2"), ("PB1", "LED2", "LED1")] {
            press(&mut b, &mut live, button, true);
            run(&mut live);
            press(&mut b, &mut live, button, false);
            run(&mut live);
            // An edit elsewhere on the board rebuilds the circuit; the flip-flop must not forget.
            let old = std::mem::replace(&mut live, Live::build(&b));
            live.keep_charge(&old);
            run(&mut live);
            let amps = |l: &str| live.reading(&b, b.part(l).unwrap()).amps;
            assert!(amps(lit) > 0.010 && amps(dark) < 1e-3, "after {button}: {lit} {} {dark} {}", amps(lit), amps(dark));
        }
    }
}
