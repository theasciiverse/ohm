//! The board: tiles, traces and parts, and the circuit they make.
//!
//! The board is the only source of truth. Every tile has four edge ports; a trace's mask joins
//! its ports, a jumper joins north with south and east with west, and a part's pins are ports on
//! its sides. Two neighbouring ports connect only when both are open toward each other.
//! Union-find over the ports gives the nets, and `Live` turns the nets into a solver circuit.
//! Nothing here draws anything; `trace_glyph` is the one place a mask becomes a character, so
//! what is drawn and what is simulated cannot drift apart.

use crate::solver::{self, Circuit, GROUND, Node, PartId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dir {
    N,
    E,
    S,
    W,
}

impl Dir {
    pub const ALL: [Dir; 4] = [Dir::N, Dir::E, Dir::S, Dir::W];

    /// A quarter turn clockwise.
    pub fn cw(self) -> Dir {
        match self {
            Dir::N => Dir::E,
            Dir::E => Dir::S,
            Dir::S => Dir::W,
            Dir::W => Dir::N,
        }
    }

    pub fn bit(self) -> u8 {
        1 << self as u8
    }

    pub fn opposite(self) -> Dir {
        match self {
            Dir::N => Dir::S,
            Dir::E => Dir::W,
            Dir::S => Dir::N,
            Dir::W => Dir::E,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Pos {
    pub x: usize,
    pub y: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Colour {
    Red,
    Yellow,
    Green,
    Blue,
}

impl Colour {
    pub fn name(self) -> &'static str {
        match self {
            Colour::Red => "red",
            Colour::Yellow => "yellow",
            Colour::Green => "green",
            Colour::Blue => "blue",
        }
    }

    /// Forward voltage at 10 mA, as the solver models it.
    pub fn vf(self) -> f64 {
        self.led().vf
    }

    fn led(self) -> solver::Led {
        match self {
            Colour::Red => solver::Led::RED,
            Colour::Yellow => solver::Led::YELLOW,
            Colour::Green => solver::Led::GREEN,
            Colour::Blue => solver::Led::BLUE,
        }
    }
}

/// What a part is. Pin `a` is the part's first pin: a battery's or supply's +, an LED's anode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Resistor { ohms: f64, watts: f64 },
    /// A filament bulb, as its hot resistance; glows by power over its rating.
    Lamp { ohms: f64, watts: f64 },
    Battery { volts: f64, r_int: f64 },
    Supply { volts: f64, limit: f64 },
    Switch { closed: bool },
    /// A push button: closed only while held; `nc`, a stop button: open only while held.
    Button { held: bool, nc: bool },
    Led(Colour),
    /// From 1 µF up an electrolytic: polarised (pin `a` is +), rated `volts`, vents if
    /// held past its surge voltage or reversed. Below 1 µF a ceramic, which never fails here.
    Capacitor { farads: f64, volts: f64 },
    /// A fixed point a job measures at; one pin.
    TestPoint,
    /// A fast-acting fuse: carries its rating, opens well above it.
    Fuse { amps: f64 },
    /// A light-dependent resistor (a GL5528-like CdS cell) in light of `lux`.
    Ldr { lux: f64 },
    /// An NTC thermistor, 10 kΩ at 25 °C, B 3950, at `celsius`.
    Ntc { celsius: f64 },
    /// A potentiometer: `ohms` end to end, the wiper (third pin) `turn` of the way from pin
    /// `a` (0) to pin `b` (1).
    Pot { ohms: f64, turn: f64 },
    /// A relay with its coil rated `volts`: the coil's legs face `facing` and its opposite, the
    /// contact's the other two sides. `nc`: the contact is closed at rest (normally closed).
    Relay { volts: f64, nc: bool },
    /// A rectifier diode (1N4001): current only from pin `a` (anode) to pin `b` (cathode, the
    /// band).
    Diode,
    /// An NPN transistor (BC547B): collector toward `facing`, emitter opposite, base a quarter
    /// turn clockwise, like a pot's wiper; `flip`: its legs bent the other way, the base a quarter
    /// turn anticlockwise.
    Npn { flip: bool },
    /// A chip in a dual-in-line package, spanning several tiles (see `Chip`).
    Chip(Chip),
}

/// A chip: which one, its pins and what they do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Chip {
    /// Four two-input NAND gates (TI's CD4011B, SCHS021D): inputs 1 and 2 to output 3, 5 and 6
    /// to 4, 8 and 9 to 10, 12 and 13 to 11; VSS (-) on pin 7, VDD (+) on pin 14.
    Cd4011,
}

impl Chip {
    pub fn name(self) -> &'static str {
        match self {
            Chip::Cd4011 => "CD4011",
        }
    }

    pub fn pins(self) -> usize {
        match self {
            Chip::Cd4011 => 14,
        }
    }

    /// Each gate as (input, input, output), pin numbers from 1.
    pub fn gates(self) -> &'static [(usize, usize, usize)] {
        match self {
            Chip::Cd4011 => &[(1, 2, 3), (5, 6, 4), (8, 9, 10), (12, 13, 11)],
        }
    }

    /// The supply pins, + then -, numbered from 1.
    pub fn supply(self) -> (usize, usize) {
        (self.pins(), self.pins() / 2)
    }

    /// What pin `n` (from 1) is, for hints and the meter.
    pub fn pin_name(self, n: usize) -> &'static str {
        const CD4011: [&str; 14] = [
            "pin 1, gate A input",
            "pin 2, gate A input",
            "pin 3, gate A output",
            "pin 4, gate B output",
            "pin 5, gate B input",
            "pin 6, gate B input",
            "pin 7, VSS (-)",
            "pin 8, gate C input",
            "pin 9, gate C input",
            "pin 10, gate C output",
            "pin 11, gate D output",
            "pin 12, gate D input",
            "pin 13, gate D input",
            "pin 14, VDD (+)",
        ];
        match self {
            Chip::Cd4011 => CD4011[n - 1],
        }
    }
}

/// A fuse's cold resistance (chosen: small next to anything it protects).
pub const FUSE_OHMS: f64 = 0.1;
/// A potentiometer's end resistance: the wiper never quite reaches zero.
const POT_END: f64 = 1.0;

/// The light levels an LDR cycles through when operated, and the temperatures for a thermistor.
pub const LUX_STEPS: [f64; 5] = [0.0, 1.0, 10.0, 100.0, 1000.0];
pub const CELSIUS_STEPS: [f64; 4] = [-18.0, 4.0, 20.0, 37.0];

impl Kind {
    /// An LDR's or thermistor's resistance as it stands, ohms.
    pub fn sensor_ohms(&self) -> Option<f64> {
        match *self {
            // GL5528: 8 to 20 kΩ at 10 lux, gamma 0.7 (the log slope from 10 to 100 lux,
            // carried on either side), at least 1 MΩ in the dark.
            Kind::Ldr { lux } => Some(if lux <= 0.0 { 1e6 } else { (15e3 * (lux / 10.0).powf(-0.7)).min(1e6) }),
            // The B equation: R = R25 exp(B (1/T - 1/T25)), temperatures in kelvin.
            Kind::Ntc { celsius } => Some(10e3 * (3950.0 * (1.0 / (celsius + 273.15) - 1.0 / 298.15)).exp()),
            _ => None,
        }
    }
}

impl Kind {
    pub fn prefix(&self) -> &'static str {
        match self {
            Kind::Resistor { .. } => "R",
            Kind::Lamp { .. } => "LP",
            Kind::Battery { .. } => "BT",
            Kind::Supply { .. } => "PSU",
            Kind::Switch { .. } => "SW",
            Kind::Button { .. } => "PB",
            Kind::Led(_) => "LED",
            Kind::Capacitor { .. } => "C",
            Kind::TestPoint => "TP",
            Kind::Fuse { .. } => "F",
            Kind::Ldr { .. } => "LDR",
            Kind::Ntc { .. } => "TH",
            Kind::Pot { .. } => "RV",
            Kind::Relay { .. } => "K",
            Kind::Diode => "D",
            Kind::Npn { .. } => "Q",
            Kind::Chip(_) => "IC",
        }
    }

    /// An electrolytic: it has a + leg.
    pub fn polarised(&self) -> bool {
        matches!(*self, Kind::Capacitor { farads, .. } if farads >= 1e-6)
    }

    pub fn is_source(&self) -> bool {
        matches!(self, Kind::Battery { .. } | Kind::Supply { .. })
    }

    pub fn pins(&self) -> usize {
        match self {
            Kind::TestPoint => 1,
            Kind::Pot { .. } | Kind::Npn { .. } => 3,
            Kind::Relay { .. } => 4,
            Kind::Chip(c) => c.pins(),
            _ => 2,
        }
    }

    /// Human names for the pins, for hints.
    pub fn pin_name(&self, pin: usize) -> &'static str {
        match (self, pin) {
            (Kind::Battery { .. } | Kind::Supply { .. }, 0) => "+ terminal",
            (Kind::Battery { .. } | Kind::Supply { .. }, _) => "- terminal",
            (Kind::Led(_), 0) => "long leg (anode, +)",
            (Kind::Led(_), _) => "short leg (cathode, -)",
            (k @ Kind::Capacitor { .. }, 0) if k.polarised() => "+ leg",
            (k @ Kind::Capacitor { .. }, _) if k.polarised() => "- leg (the striped side)",
            (Kind::Pot { .. }, 0) => "first end",
            (Kind::Pot { .. }, 1) => "other end",
            (Kind::Pot { .. }, _) => "wiper (middle leg)",
            (Kind::Diode, 0) => "anode (+)",
            (Kind::Diode, _) => "cathode (-, the band)",
            (Kind::Npn { .. }, 0) => "collector (C)",
            (Kind::Npn { .. }, 1) => "emitter (E)",
            (Kind::Npn { .. }, _) => "base (B)",
            (Kind::Relay { .. }, 0 | 1) => "coil leg",
            (Kind::Relay { .. }, _) => "contact leg",
            (Kind::Chip(c), n) => c.pin_name(n + 1),
            (_, 0) => "first leg",
            _ => "second leg",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub kind: Kind,
    pub pos: Pos,
    /// The side pin `a` faces; pin `b` faces the opposite side.
    pub facing: Dir,
    pub label: String,
    /// Glued down by the job: can't be moved or erased.
    pub fixed: bool,
    /// Burnt open; stays so until mended.
    pub burnt: bool,
}

impl Part {
    /// Tiles across and down. A chip stands as in its datasheet's top view when it faces
    /// north (the notch at the top: pins 1 to n/2 down the left, the rest up the right), two
    /// tiles wide; every other part takes one tile.
    pub fn size(&self) -> (usize, usize) {
        footprint(self.kind, self.facing)
    }

    /// The tiles the part covers, its corner `pos` first.
    pub fn tiles(&self) -> Vec<Pos> {
        let (w, h) = self.size();
        (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).map(|(x, y)| Pos { x: self.pos.x + x, y: self.pos.y + y }).collect()
    }

    pub fn covers(&self, p: Pos) -> bool {
        let (w, h) = self.size();
        p.x >= self.pos.x && p.x < self.pos.x + w && p.y >= self.pos.y && p.y < self.pos.y + h
    }

    /// Each pin's tile and the side it faces, in pin order.
    pub fn pin_ports(&self) -> Vec<(Pos, Dir)> {
        let Kind::Chip(chip) = self.kind else { return self.pin_sides().into_iter().map(|d| (self.pos, d)).collect() };
        let half = chip.pins() / 2;
        // Facing north: pin k (from 1) down the left side, then back up the right.
        let north = (0..chip.pins()).map(|k| if k < half { (0, k, Dir::W) } else { (1, chip.pins() - 1 - k, Dir::E) });
        let turns = match self.facing {
            Dir::N => 0,
            Dir::E => 1,
            Dir::S => 2,
            Dir::W => 3,
        };
        north
            .map(|(mut x, mut y, mut d)| {
                let (mut w, mut h) = (2, half);
                for _ in 0..turns {
                    (x, y, d) = (h - 1 - y, x, d.cw());
                    (w, h) = (h, w);
                }
                let _ = w;
                (Pos { x: self.pos.x + x, y: self.pos.y + y }, d)
            })
            .collect()
    }

    /// The side each pin faces, for a part on one tile.
    pub fn pin_sides(&self) -> Vec<Dir> {
        match self.kind.pins() {
            1 => vec![self.facing],
            // A potentiometer's wiper is a quarter turn clockwise from pin `a`.
            3 if matches!(self.kind, Kind::Npn { flip: true }) => vec![self.facing, self.facing.opposite(), self.facing.cw().opposite()],
            3 => vec![self.facing, self.facing.opposite(), self.facing.cw()],
            // A relay: the coil along `facing`, the contact across it.
            4 => vec![self.facing, self.facing.opposite(), self.facing.cw(), self.facing.cw().opposite()],
            _ => vec![self.facing, self.facing.opposite()],
        }
    }
}

/// Tiles across and down for a part of this kind standing this way.
pub fn footprint(kind: Kind, facing: Dir) -> (usize, usize) {
    match kind {
        Kind::Chip(c) if matches!(facing, Dir::N | Dir::S) => (2, c.pins() / 2),
        Kind::Chip(c) => (c.pins() / 2, 2),
        _ => (1, 1),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Empty,
    /// Outside the board's shape.
    Blocked,
    /// Copper: the bits of the sides it reaches.
    Trace(u8),
    /// Two traces crossing without touching: north-south and east-west.
    Jumper,
    Part(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Board {
    pub w: usize,
    pub h: usize,
    cells: Vec<Cell>,
    pub parts: Vec<Part>,
}

impl Board {
    pub fn new(w: usize, h: usize) -> Self {
        Board { w, h, cells: vec![Cell::Empty; w * h], parts: Vec::new() }
    }

    pub fn cell(&self, p: Pos) -> Cell {
        self.cells[p.y * self.w + p.x]
    }

    fn set(&mut self, p: Pos, c: Cell) {
        self.cells[p.y * self.w + p.x] = c;
    }

    pub fn neighbour(&self, p: Pos, d: Dir) -> Option<Pos> {
        match d {
            Dir::N if p.y > 0 => Some(Pos { x: p.x, y: p.y - 1 }),
            Dir::S if p.y + 1 < self.h => Some(Pos { x: p.x, y: p.y + 1 }),
            Dir::W if p.x > 0 => Some(Pos { x: p.x - 1, y: p.y }),
            Dir::E if p.x + 1 < self.w => Some(Pos { x: p.x + 1, y: p.y }),
            _ => None,
        }
    }

    pub fn part(&self, label: &str) -> Option<usize> {
        self.parts.iter().position(|p| p.label == label)
    }

    /// Is the port on side `d` of tile `p` open?
    pub fn open(&self, p: Pos, d: Dir) -> bool {
        match self.cell(p) {
            Cell::Trace(m) => m & d.bit() != 0,
            Cell::Jumper => true,
            Cell::Part(i) => self.parts[i].pin_ports().contains(&(p, d)),
            Cell::Empty | Cell::Blocked => false,
        }
    }

    /// Can a part of this kind stand here, its corner at `pos`: every tile it needs on the
    /// board and empty (or held by part `except`, for turning a part where it stands)?
    pub fn fits(&self, kind: Kind, pos: Pos, facing: Dir, except: Option<usize>) -> bool {
        let (w, h) = footprint(kind, facing);
        pos.x + w <= self.w
            && pos.y + h <= self.h
            && (0..h).all(|y| {
                (0..w).all(|x| match self.cell(Pos { x: pos.x + x, y: pos.y + y }) {
                    Cell::Empty => true,
                    Cell::Part(i) => Some(i) == except,
                    _ => false,
                })
            })
    }

    /// Places a part on empty tiles, its corner at `pos`, labelled with the next free number
    /// for its kind.
    pub fn place(&mut self, kind: Kind, pos: Pos, facing: Dir, fixed: bool) -> Option<usize> {
        if !self.fits(kind, pos, facing, None) {
            return None;
        }
        let prefix = kind.prefix();
        let n = (1..).find(|n| self.part(&format!("{prefix}{n}")).is_none()).unwrap();
        self.parts.push(Part { kind, pos, facing, label: format!("{prefix}{n}"), fixed, burnt: false });
        let i = self.parts.len() - 1;
        for t in self.parts[i].tiles() {
            self.set(t, Cell::Part(i));
        }
        Some(i)
    }

    /// Turns part `i` to face `facing` where it stands, if it fits that way.
    pub fn turn(&mut self, i: usize, facing: Dir) -> bool {
        let part = &self.parts[i];
        if !self.fits(part.kind, part.pos, facing, Some(i)) {
            return false;
        }
        for t in self.parts[i].tiles() {
            self.set(t, Cell::Empty);
        }
        self.parts[i].facing = facing;
        for t in self.parts[i].tiles() {
            self.set(t, Cell::Part(i));
        }
        true
    }

    /// Lays copper from tile `p` toward side `d`, on both tiles. Parts keep their pins: copper
    /// only reaches into a trace, a jumper, or an empty tile.
    pub fn connect(&mut self, p: Pos, d: Dir) -> bool {
        let Some(q) = self.neighbour(p, d) else { return false };
        let reach = |b: &mut Board, at: Pos, side: Dir| match b.cell(at) {
            Cell::Empty => {
                b.set(at, Cell::Trace(side.bit()));
                true
            }
            Cell::Trace(m) => {
                b.set(at, Cell::Trace(m | side.bit()));
                true
            }
            Cell::Jumper => true,
            Cell::Part(i) => b.parts[i].pin_ports().contains(&(at, side)),
            Cell::Blocked => false,
        };
        let can = |b: &Board, at: Pos, side: Dir| match b.cell(at) {
            Cell::Empty | Cell::Trace(_) | Cell::Jumper => true,
            Cell::Part(i) => b.parts[i].pin_ports().contains(&(at, side)),
            Cell::Blocked => false,
        };
        if !can(self, p, d) || !can(self, q, d.opposite()) {
            return false;
        }
        reach(self, p, d);
        reach(self, q, d.opposite());
        true
    }

    /// Clears a tile, unless a fixed part sits on it. The removed part's label is freed.
    pub fn erase(&mut self, p: Pos) -> bool {
        match self.cell(p) {
            Cell::Part(i) if self.parts[i].fixed => false,
            Cell::Part(i) => {
                for t in self.parts[i].tiles() {
                    self.set(t, Cell::Empty);
                }
                self.parts.remove(i);
                for c in &mut self.cells {
                    if let Cell::Part(j) = c
                        && *j > i
                    {
                        *j -= 1;
                    }
                }
                true
            }
            Cell::Trace(_) | Cell::Jumper => {
                self.set(p, Cell::Empty);
                true
            }
            Cell::Empty | Cell::Blocked => false,
        }
    }

    /// Turns a four-way junction into a jumper crossing, or back.
    pub fn toggle_jumper(&mut self, p: Pos) -> bool {
        match self.cell(p) {
            Cell::Trace(0b1111) => self.set(p, Cell::Jumper),
            Cell::Jumper => self.set(p, Cell::Trace(0b1111)),
            _ => return false,
        }
        true
    }

    /// Sets a tile that holds no part: copper, a jumper, blocked or empty (for loading saves).
    pub fn set_plain(&mut self, p: Pos, c: Cell) {
        if !matches!(c, Cell::Part(_)) && !matches!(self.cell(p), Cell::Part(_)) {
            self.set(p, c);
        }
    }

    /// This board laid in the middle of a bigger empty one, `w` by `h` tiles: the workbench a
    /// job's board sits on. Returns the bigger board and where this one's corner landed.
    pub fn embed(&self, w: usize, h: usize) -> (Board, Pos) {
        let (w, h) = (w.max(self.w), h.max(self.h));
        let off = Pos { x: (w - self.w) / 2, y: (h - self.h) / 2 };
        let mut big = Board::new(w, h);
        for y in 0..self.h {
            for x in 0..self.w {
                let c = self.cell(Pos { x, y });
                if !matches!(c, Cell::Part(_)) {
                    big.set(Pos { x: x + off.x, y: y + off.y }, c);
                }
            }
        }
        for p in &self.parts {
            let at = Pos { x: p.pos.x + off.x, y: p.pos.y + off.y };
            let i = big.place(p.kind, at, p.facing, p.fixed).expect("embedding onto an empty tile");
            big.parts[i].label = p.label.clone();
            big.parts[i].burnt = p.burnt;
        }
        (big, off)
    }

    /// A board from text, one character per tile: box-drawing traces (`─│┌┐└┘├┤┬┴┼` and the
    /// stubs `╴╶╵╷`), `x` a jumper, `#` blocked, space or `.` empty, and any other character a
    /// part from `legend` as (character, kind, side pin `a` faces). Parts are labelled in
    /// reading order.
    pub fn parse(rows: &[&str], legend: &[(char, Kind, Dir)]) -> Result<Board, String> {
        let h = rows.len();
        let w = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
        let mut b = Board::new(w, h);
        for (y, row) in rows.iter().enumerate() {
            for (x, ch) in row.chars().enumerate() {
                let p = Pos { x, y };
                match ch {
                    ' ' | '.' => {}
                    '#' => b.set(p, Cell::Blocked),
                    'x' => b.set(p, Cell::Jumper),
                    _ => {
                        if let Some(m) = trace_mask(ch) {
                            b.set(p, Cell::Trace(m));
                        } else if let Some((_, kind, facing)) = legend.iter().find(|l| l.0 == ch) {
                            // A chip is drawn by repeating its character over its tiles: the
                            // first one in reading order is its corner.
                            if let Cell::Part(i) = b.cell(p)
                                && b.parts[i].kind == *kind
                            {
                                continue;
                            }
                            if b.place(*kind, p, *facing, false).is_none() {
                                return Err(format!("no room for {ch:?} at {x},{y}"));
                            }
                        } else {
                            return Err(format!("unknown tile {ch:?} at {x},{y}"));
                        }
                    }
                }
            }
        }
        Ok(b)
    }
}

/// The glyph for a trace mask; the one place copper becomes a character.
pub fn trace_glyph(mask: u8) -> char {
    const N: u8 = 1;
    const E: u8 = 2;
    const S: u8 = 4;
    const W: u8 = 8;
    match mask {
        0 => ' ',
        m if m == N => '╵',
        m if m == E => '╶',
        m if m == S => '╷',
        m if m == W => '╴',
        m if m == E | W => '─',
        m if m == N | S => '│',
        m if m == E | S => '┌',
        m if m == W | S => '┐',
        m if m == N | E => '└',
        m if m == N | W => '┘',
        m if m == N | S | E => '├',
        m if m == N | S | W => '┤',
        m if m == E | W | S => '┬',
        m if m == E | W | N => '┴',
        _ => '┼',
    }
}

fn trace_mask(ch: char) -> Option<u8> {
    (1..16u8).find(|&m| trace_glyph(m) == ch)
}

/// The trace mask a glyph from `trace_glyph` stands for.
pub fn trace_mask_of(ch: char) -> Option<u8> {
    trace_mask(ch)
}

/// Which net every port and every pin is on.
#[derive(Clone, Debug)]
pub struct Netlist {
    pub nets: usize,
    port_net: Vec<Option<usize>>,
    /// Per part, the net of each pin.
    pub pin_net: Vec<Vec<usize>>,
    /// Open trace ends that meet nothing.
    pub dangling: Vec<(Pos, Dir)>,
}

impl Netlist {
    pub fn build(b: &Board) -> Netlist {
        let port = |p: Pos, d: Dir| (p.y * b.w + p.x) * 4 + d as usize;
        let mut uf = UnionFind::new(b.w * b.h * 4);
        let mut dangling = Vec::new();
        for y in 0..b.h {
            for x in 0..b.w {
                let p = Pos { x, y };
                match b.cell(p) {
                    Cell::Trace(m) => {
                        let sides: Vec<Dir> = Dir::ALL.into_iter().filter(|d| m & d.bit() != 0).collect();
                        for pair in sides.windows(2) {
                            uf.union(port(p, pair[0]), port(p, pair[1]));
                        }
                    }
                    Cell::Jumper => {
                        uf.union(port(p, Dir::N), port(p, Dir::S));
                        uf.union(port(p, Dir::E), port(p, Dir::W));
                    }
                    _ => {}
                }
                for d in Dir::ALL {
                    if !b.open(p, d) {
                        continue;
                    }
                    match b.neighbour(p, d) {
                        Some(q) if b.open(q, d.opposite()) => uf.union(port(p, d), port(q, d.opposite())),
                        _ => {
                            if !matches!(b.cell(p), Cell::Part(_)) {
                                dangling.push((p, d));
                            }
                        }
                    }
                }
            }
        }
        let mut id_of_root = vec![None; b.w * b.h * 4];
        let mut port_net = vec![None; b.w * b.h * 4];
        let mut nets = 0;
        for y in 0..b.h {
            for x in 0..b.w {
                let p = Pos { x, y };
                for d in Dir::ALL {
                    if b.open(p, d) {
                        let r = uf.find(port(p, d));
                        let id = *id_of_root[r].get_or_insert_with(|| {
                            nets += 1;
                            nets - 1
                        });
                        port_net[port(p, d)] = Some(id);
                    }
                }
            }
        }
        let pin_net = b
            .parts
            .iter()
            .map(|part| part.pin_ports().iter().map(|&(p, d)| port_net[port(p, d)].unwrap()).collect())
            .collect();
        Netlist { nets, port_net, pin_net, dangling }
    }

    /// The net on side `d` of tile `p`, if that port is open.
    pub fn net_at(&self, b: &Board, p: Pos, d: Dir) -> Option<usize> {
        self.port_net[(p.y * b.w + p.x) * 4 + d as usize]
    }

    /// Every (part, pin) on a net.
    pub fn pins_on(&self, net: usize) -> Vec<(usize, usize)> {
        let mut v = Vec::new();
        for (part, nets) in self.pin_net.iter().enumerate() {
            for (pin, &n) in nets.iter().enumerate() {
                if n == net {
                    v.push((part, pin));
                }
            }
        }
        v
    }
}

struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        UnionFind { parent: (0..n).collect() }
    }

    fn find(&mut self, mut k: usize) -> usize {
        while self.parent[k] != k {
            self.parent[k] = self.parent[self.parent[k]];
            k = self.parent[k];
        }
        k
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[ra] = rb;
        }
    }
}

/// A bench supply's output resistance, so a dead short across its terminals still solves.
const SUPPLY_R_OUT: f64 = 0.001;

/// What a part is doing right now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    /// Voltage from pin `a` to pin `b` (a test point: to ground).
    pub volts: f64,
    /// Current from `a` to `b` through the part; a source's: what it delivers.
    pub amps: f64,
    pub watts: f64,
    /// Load against the rating, see `solver::Circuit::heat`.
    pub heat: f64,
    pub burnt: bool,
}

/// A board turned into a running circuit. Ground is the first source's - terminal, so every
/// voltage reads as a multimeter with its black probe there would.
pub struct Live {
    pub net: Netlist,
    pub sim: Circuit,
    nodes: Vec<Node>,
    ids: Vec<Option<PartId>>,
    /// A part's second element: a potentiometer's second half (wiper to `b`, its first half
    /// is in `ids`), a relay's contact (its coil is in `ids`).
    lower: Vec<Option<PartId>>,
    /// Every capacitor's, relay's and transistor's tile and part index, to carry its charge or
    /// its state over a rebuild.
    caps: Vec<(Pos, usize)>,
    /// Each part's kind, to match what is carried over.
    parts_kind: Vec<Kind>,
    /// A chip's gates, in the chip's order (the first is also in `ids`).
    gates: Vec<Vec<PartId>>,
}

impl Live {
    pub fn build(b: &Board) -> Live {
        let net = Netlist::build(b);
        let ground = b.parts.iter().enumerate().find(|(_, p)| p.kind.is_source()).map(|(i, _)| net.pin_net[i][1]);
        let mut sim = Circuit::new();
        let nodes: Vec<Node> = (0..net.nets).map(|n| if Some(n) == ground { GROUND } else { sim.node() }).collect();
        let mut ids = Vec::new();
        let mut lower = Vec::new();
        let mut caps = Vec::new();
        let mut gates = Vec::new();
        for (i, part) in b.parts.iter().enumerate() {
            lower.push(None);
            gates.push(Vec::new());
            let pin = |k: usize| nodes[net.pin_net[i][k]];
            let id = match part.kind {
                Kind::Resistor { ohms, watts } | Kind::Lamp { ohms, watts } => {
                    Some(sim.resistor_rated(pin(0), pin(1), ohms, watts))
                }
                Kind::Battery { volts, r_int } => Some(sim.battery(pin(0), pin(1), volts, r_int)),
                Kind::Supply { volts, limit } => {
                    let inner = sim.node();
                    sim.resistor(pin(0), inner, SUPPLY_R_OUT);
                    Some(sim.supply(inner, pin(1), volts, limit))
                }
                Kind::Switch { closed } => Some(sim.switch(pin(0), pin(1), closed)),
                Kind::Button { held, nc } => Some(sim.switch(pin(0), pin(1), held != nc)),
                Kind::Led(c) => Some(sim.led(pin(0), pin(1), c.led())),
                Kind::Capacitor { farads, volts } => {
                    caps.push((part.pos, i));
                    Some(if part.kind.polarised() { sim.electrolytic(pin(0), pin(1), farads, volts) } else { sim.capacitor(pin(0), pin(1), farads) })
                }
                Kind::TestPoint => None,
                Kind::Fuse { amps } => Some(sim.fuse(pin(0), pin(1), FUSE_OHMS, amps)),
                // GL5528: 100 mW.
                Kind::Ldr { .. } => Some(sim.resistor_rated(pin(0), pin(1), part.kind.sensor_ohms().unwrap(), 0.1)),
                Kind::Ntc { .. } => Some(sim.resistor(pin(0), pin(1), part.kind.sensor_ohms().unwrap())),
                Kind::Pot { ohms, turn } => {
                    let turn = turn.clamp(0.0, 1.0);
                    lower[i] = Some(sim.resistor(pin(2), pin(1), (ohms * (1.0 - turn)).max(POT_END)));
                    Some(sim.resistor(pin(0), pin(2), (ohms * turn).max(POT_END)))
                }
                Kind::Diode => Some(sim.led(pin(0), pin(1), solver::Led::N4001)),
                Kind::Npn { .. } => {
                    caps.push((part.pos, i));
                    Some(sim.npn(pin(0), pin(2), pin(1), solver::Npn::BC547B))
                }
                Kind::Relay { volts, nc } => {
                    caps.push((part.pos, i));
                    let coil = sim.relay(pin(0), pin(1), pin(2), pin(3), volts, nc);
                    lower[i] = sim.contact(coil);
                    Some(coil)
                }
                Kind::Chip(chip) => {
                    caps.push((part.pos, i));
                    let (vdd, vss) = chip.supply();
                    let (vdd, vss) = (pin(vdd - 1), pin(vss - 1));
                    let mut first = None;
                    for &(a, bb, y) in chip.gates() {
                        let g = sim.nand(pin(y - 1), pin(a - 1), pin(bb - 1), vdd, vss, first);
                        first.get_or_insert(g);
                        gates[i].push(g);
                    }
                    for &(a, bb, _) in chip.gates() {
                        for k in [a, bb] {
                            sim.capacitor(pin(k - 1), vss, solver::CMOS_INPUT_FARADS);
                        }
                    }
                    first
                }
            };
            if part.burnt {
                for id in id.iter().chain(&gates[i]) {
                    sim.set_burnt(*id, true);
                }
            }
            ids.push(id);
        }
        Live { net, sim, nodes, ids, lower, caps, parts_kind: b.parts.iter().map(|p| p.kind).collect(), gates }
    }

    /// Keeps the charge of every capacitor, and the state of every relay and transistor, that
    /// sat on the same tile in `old`, so editing the board or turning a knob doesn't empty a
    /// capacitor, drop a relay or make a flip-flop forget.
    pub fn keep_charge(&mut self, old: &Live) {
        for &(pos, i) in &self.caps {
            if let Some(&(_, j)) = old.caps.iter().find(|(p, _)| *p == pos)
                && let (Some(id), Some(was)) = (self.ids[i], old.ids[j])
            {
                let (new, prev) = (&self.parts_kind[i], &old.parts_kind[j]);
                match (new, prev) {
                    (Kind::Relay { .. }, Kind::Relay { .. }) => self.sim.set_energised(id, old.sim.energised(was)),
                    (Kind::Npn { .. }, Kind::Npn { .. }) => self.sim.set_junctions(id, old.sim.junctions(was)),
                    (Kind::Capacitor { .. }, Kind::Capacitor { .. }) => self.sim.set_cap_voltage(id, old.sim.cap_voltage(was)),
                    (Kind::Chip(a), Kind::Chip(b)) if a == b => {
                        for (g, h) in self.gates[i].iter().zip(&old.gates[j]) {
                            self.sim.set_gate_high(*g, old.sim.gate_high(*h));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// A relay has pulled in.
    pub fn energised(&self, part: usize) -> bool {
        self.ids[part].is_some_and(|id| self.sim.energised(id))
    }

    /// Flips a switch or a button in the running circuit, without rebuilding it.
    pub fn set_switch(&mut self, part: usize, closed: bool) {
        if let Some(id) = self.ids[part] {
            self.sim.set_switch(id, closed);
        }
    }

    /// Current through every copper tile: for each tile and side, the amps leaving the tile
    /// through that side (negative: arriving). Copper has no resistance, so where it forms a loop
    /// the split is arbitrary; the flow is found on a spanning tree of each net's copper, which
    /// is exact for copper without loops.
    pub fn copper_currents(&self, b: &Board) -> Vec<[f64; 4]> {
        let tile = |p: Pos| p.y * b.w + p.x;
        let mut out = vec![[0.0; 4]; b.w * b.h];
        // Copper nodes: a trace is one node, a jumper two (north-south, east-west).
        let node = |p: Pos, d: Dir| -> Option<usize> {
            match b.cell(p) {
                Cell::Trace(m) if m & d.bit() != 0 => Some(tile(p) * 2),
                Cell::Jumper => Some(tile(p) * 2 + usize::from(matches!(d, Dir::E | Dir::W))),
                _ => None,
            }
        };
        let n = b.w * b.h * 2;
        let mut inject = vec![0.0; n];
        let mut adj: Vec<Vec<(usize, Pos, Dir)>> = vec![Vec::new(); n];
        for y in 0..b.h {
            for x in 0..b.w {
                let p = Pos { x, y };
                for d in [Dir::E, Dir::S] {
                    if let (Some(a), Some(q)) = (node(p, d), b.neighbour(p, d))
                        && let Some(c) = node(q, d.opposite())
                    {
                        adj[a].push((c, p, d));
                        adj[c].push((a, q, d.opposite()));
                    }
                }
            }
        }
        // Each part pin feeds its current into the copper tile it faces.
        for (i, part) in b.parts.iter().enumerate() {
            let r = self.reading(b, i);
            if part.kind.pins() < 2 || r.burnt {
                continue;
            }
            let into_net: Vec<f64> = if let Kind::Chip(chip) = part.kind {
                // A chip: each gate's output current out of its pin, drawn from + or sunk to -.
                let mut v = vec![0.0; chip.pins()];
                let (vdd, vss) = chip.supply();
                for (&(_, _, y), &g) in chip.gates().iter().zip(&self.gates[i]) {
                    let amps = self.sim.current(g);
                    v[y - 1] += amps;
                    v[if self.sim.gate_high(g) { vdd } else { vss } - 1] -= amps;
                }
                v
            } else if part.kind.is_source() {
                vec![r.amps, -r.amps]
            } else if let (Kind::Npn { .. }, Some(id)) = (part.kind, self.ids[i]) {
                // A transistor: collector and base currents in, their sum out of the emitter.
                let ib = self.sim.base_current(id);
                vec![-r.amps, r.amps + ib, -ib]
            } else if let (Kind::Relay { .. }, Some(contact)) = (part.kind, self.lower[i]) {
                // A relay: the coil's current through its legs, the contact's through its own.
                let ic = self.sim.current(contact);
                vec![-r.amps, r.amps, -ic, ic]
            } else if let Some(low) = self.lower[i] {
                // A potentiometer: in at one end, out at the other, the difference at the wiper.
                let i2 = self.sim.current(low);
                vec![-r.amps, i2, r.amps - i2]
            } else {
                vec![-r.amps, r.amps]
            };
            for (k, &(at, d)) in part.pin_ports().iter().enumerate() {
                if let Some(q) = b.neighbour(at, d)
                    && let Some(c) = node(q, d.opposite())
                {
                    inject[c] += into_net[k];
                    out[tile(q)][d.opposite() as usize] -= into_net[k];
                }
            }
        }
        // Spanning forest by breadth-first search, then flows from the leaves up.
        let mut seen = vec![false; n];
        for root in 0..n {
            if seen[root] || adj[root].is_empty() && inject[root] == 0.0 {
                continue;
            }
            seen[root] = true;
            let mut order = vec![root];
            let mut parent: Vec<Option<(usize, Pos, Dir)>> = vec![None; n];
            let mut k = 0;
            while k < order.len() {
                let a = order[k];
                k += 1;
                for &(c, p, d) in &adj[a] {
                    if !seen[c] {
                        seen[c] = true;
                        // (parent node, the child's tile, the child's side toward the parent)
                        parent[c] = Some((a, b.neighbour(p, d).unwrap(), d.opposite()));
                        order.push(c);
                    }
                }
            }
            let mut sum: Vec<f64> = vec![0.0; n];
            for &a in &order {
                sum[a] = inject[a];
            }
            for &a in order.iter().rev() {
                if let Some((up, q, d)) = parent[a] {
                    let f = sum[a];
                    sum[up] += f;
                    out[tile(q)][d as usize] += f;
                    let pq = b.neighbour(q, d).unwrap();
                    out[tile(pq)][d.opposite() as usize] -= f;
                }
            }
        }
        out
    }

    pub fn net_volts(&self, net: usize) -> f64 {
        self.sim.voltage(self.nodes[net])
    }

    /// A chip's gates are high.
    pub fn gates_high(&self, part: usize) -> Vec<bool> {
        self.gates[part].iter().map(|&g| self.sim.gate_high(g)).collect()
    }

    /// The voltage on pin `k` (from 0) of a part, from ground.
    pub fn pin_volts(&self, part: usize, k: usize) -> f64 {
        self.net_volts(self.net.pin_net[part][k])
    }

    pub fn reading(&self, b: &Board, part: usize) -> Reading {
        let nets = &self.net.pin_net[part];
        let v = |k: usize| self.net_volts(nets[k]);
        if let Kind::Chip(chip) = b.parts[part].kind {
            // A chip: its supply voltage, the current it draws from + (what its high outputs
            // source), the heat of its hottest gate.
            let (vdd, vss) = chip.supply();
            let gates = &self.gates[part];
            let amps: f64 = gates.iter().filter(|&&g| self.sim.gate_high(g)).map(|&g| self.sim.current(g).max(0.0)).sum();
            return Reading {
                volts: v(vdd - 1) - v(vss - 1),
                amps: if amps < 1e-9 { 0.0 } else { amps },
                watts: gates.iter().map(|&g| self.sim.power(g)).sum(),
                heat: gates.iter().map(|&g| self.sim.heat(g)).fold(0.0, f64::max),
                burnt: gates.iter().any(|&g| self.sim.burnt(g)),
            };
        }
        let volts = if b.parts[part].kind.pins() == 1 { v(0) } else { v(0) - v(1) };
        // Below a nanoamp is the solver's tiny ground leak, not current: read it as none.
        let quiet = |v: f64| if v.abs() < 1e-9 { 0.0 } else { v };
        match self.ids[part] {
            Some(id) => Reading {
                volts,
                amps: quiet(self.sim.current(id)),
                watts: if self.sim.current(id).abs() < 1e-9 { 0.0 } else { self.sim.power(id) },
                heat: self.sim.heat(id),
                burnt: self.sim.burnt(id),
            },
            None => Reading { volts, amps: 0.0, watts: 0.0, heat: 0.0, burnt: false },
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A torch: battery on the left (+ up), a resistor along the top, an LED down the right
    /// (anode up), a switch along the bottom.
    pub(crate) fn torch() -> Board {
        Board::parse(
            &["┌R─┐", "B  L", "└S─┘"],
            &[
                ('B', Kind::Battery { volts: 4.5, r_int: 0.45 }, Dir::N),
                ('R', Kind::Resistor { ohms: 220.0, watts: 0.25 }, Dir::W),
                ('L', Kind::Led(Colour::Red), Dir::N),
                ('S', Kind::Switch { closed: true }, Dir::W),
            ],
        )
        .unwrap()
    }

    #[test]
    fn embedding_keeps_parts_labels_and_circuit() {
        let (big, off) = torch().embed(20, 11);
        assert_eq!((big.w, big.h, off), (20, 11, Pos { x: 8, y: 4 }));
        let labels: Vec<&str> = big.parts.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(labels, ["R1", "BT1", "LED1", "SW1"]);
        let mut live = Live::build(&big);
        live.sim.solve_dc().unwrap();
        let led = live.reading(&big, big.part("LED1").unwrap());
        assert!((led.amps - 0.01126).abs() < 0.0002, "{led:?}");
    }

    #[test]
    fn every_glyph_round_trips() {
        for m in 1..16u8 {
            assert_eq!(trace_mask(trace_glyph(m)), Some(m), "mask {m:04b}");
        }
    }

    #[test]
    fn torch_makes_one_loop() {
        let b = torch();
        let n = Netlist::build(&b);
        // Battery+ to R1, R1 to LED anode, LED cathode to SW1, SW1 to battery-.
        assert_eq!(n.nets, 4);
        assert!(n.dangling.is_empty());
        let labels: Vec<&str> = b.parts.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(labels, ["R1", "BT1", "LED1", "SW1"]);
        let bt = b.part("BT1").unwrap();
        let r = b.part("R1").unwrap();
        assert_eq!(n.pin_net[bt][0], n.pin_net[r][0]);
    }

    #[test]
    fn torch_lights_its_led() {
        let b = torch();
        let mut live = Live::build(&b);
        live.sim.solve_dc().unwrap();
        let led = live.reading(&b, b.part("LED1").unwrap());
        assert!((led.amps - 0.01126).abs() < 0.0002, "{led:?}");
    }

    #[test]
    fn crossing_traces_stay_apart_on_a_jumper_and_join_on_a_junction() {
        let legend = [
            ('A', Kind::TestPoint, Dir::S),
            ('B', Kind::TestPoint, Dir::N),
            ('C', Kind::TestPoint, Dir::E),
            ('D', Kind::TestPoint, Dir::W),
        ];
        let jumper = Board::parse(&[" A ", "CxD", " B "], &legend).unwrap();
        let n = Netlist::build(&jumper);
        let net = |b: &Board, n: &Netlist, l: &str| n.pin_net[b.part(l).unwrap()][0];
        assert_eq!(net(&jumper, &n, "TP1"), net(&jumper, &n, "TP4"));
        assert_ne!(net(&jumper, &n, "TP1"), net(&jumper, &n, "TP2"));

        let mut junction = jumper.clone();
        assert!(junction.toggle_jumper(Pos { x: 1, y: 1 }));
        let n = Netlist::build(&junction);
        assert_eq!(n.nets, 1);
    }

    #[test]
    fn a_trace_ending_in_the_air_is_dangling() {
        // The second trace reaches east into an empty tile. A stub (`╴`) ends cleanly.
        let b = Board::parse(&["A── ", "A─╴ "], &[('A', Kind::TestPoint, Dir::E)]).unwrap();
        let n = Netlist::build(&b);
        assert_eq!(n.dangling, vec![(Pos { x: 2, y: 0 }, Dir::E)]);
    }

    #[test]
    fn copper_only_meets_a_part_at_its_pins() {
        let mut b = Board::new(3, 3);
        b.place(Kind::Resistor { ohms: 100.0, watts: 0.25 }, Pos { x: 1, y: 1 }, Dir::W, false);
        // Its pins face west and east: north is a wall.
        assert!(!b.connect(Pos { x: 1, y: 0 }, Dir::S));
        assert!(b.connect(Pos { x: 0, y: 1 }, Dir::E));
        assert_eq!(b.cell(Pos { x: 0, y: 1 }), Cell::Trace(Dir::E.bit()));
    }

    #[test]
    fn erasing_frees_the_label_but_not_a_fixed_part() {
        let mut b = Board::new(3, 1);
        b.place(Kind::Resistor { ohms: 100.0, watts: 0.25 }, Pos { x: 0, y: 0 }, Dir::W, false);
        b.place(Kind::Resistor { ohms: 220.0, watts: 0.25 }, Pos { x: 1, y: 0 }, Dir::W, false);
        b.place(Kind::Battery { volts: 9.0, r_int: 1.0 }, Pos { x: 2, y: 0 }, Dir::W, true);
        assert!(b.erase(Pos { x: 0, y: 0 }));
        assert!(!b.erase(Pos { x: 2, y: 0 }));
        assert_eq!(b.cell(Pos { x: 1, y: 0 }), Cell::Part(0));
        let again = b.place(Kind::Resistor { ohms: 470.0, watts: 0.25 }, Pos { x: 0, y: 0 }, Dir::W, false).unwrap();
        assert_eq!(b.parts[again].label, "R1");
    }

    #[test]
    fn copper_carries_the_loop_current_round_the_torch() {
        let b = torch();
        let mut live = Live::build(&b);
        live.sim.solve_dc().unwrap();
        let i = live.reading(&b, b.part("LED1").unwrap()).amps;
        let flow = live.copper_currents(&b);
        let at = |x: usize, y: usize| flow[y * b.w + x];
        // Top-left corner: in from the battery below, out east to R1.
        assert!((at(0, 0)[Dir::E as usize] - i).abs() < 1e-6, "{:?}", at(0, 0));
        assert!((at(0, 0)[Dir::S as usize] + i).abs() < 1e-6, "{:?}", at(0, 0));
        // Bottom row runs west, back to the battery's - terminal.
        assert!((at(2, 2)[Dir::W as usize] - i).abs() < 1e-6, "{:?}", at(2, 2));
    }

    #[test]
    fn a_chip_stands_as_in_its_datasheet_and_turns() {
        let chip = Kind::Chip(Chip::Cd4011);
        let mut b = Board::new(8, 8);
        let i = b.place(chip, Pos { x: 0, y: 0 }, Dir::N, false).unwrap();
        // Notch up: pin 1 top left facing west, pin 7 bottom left, 8 bottom right, 14 top right.
        let ports = b.parts[i].pin_ports();
        assert_eq!(ports[0], (Pos { x: 0, y: 0 }, Dir::W));
        assert_eq!(ports[6], (Pos { x: 0, y: 6 }, Dir::W));
        assert_eq!(ports[7], (Pos { x: 1, y: 6 }, Dir::E));
        assert_eq!(ports[13], (Pos { x: 1, y: 0 }, Dir::E));
        assert_eq!(b.parts[i].tiles().len(), 14);
        // Notch west: pins 1 to 7 along the bottom, left to right, 14 at the top left.
        let mut b = Board::new(8, 8);
        let i = b.place(chip, Pos { x: 0, y: 0 }, Dir::W, false).unwrap();
        let ports = b.parts[i].pin_ports();
        assert_eq!(ports[0], (Pos { x: 0, y: 1 }, Dir::S));
        assert_eq!(ports[6], (Pos { x: 6, y: 1 }, Dir::S));
        assert_eq!(ports[7], (Pos { x: 6, y: 0 }, Dir::N));
        assert_eq!(ports[13], (Pos { x: 0, y: 0 }, Dir::N));
        // Every tile it covers is its own, and erasing any of them takes it all away.
        assert_eq!(b.cell(Pos { x: 6, y: 1 }), Cell::Part(i));
        assert!(b.erase(Pos { x: 4, y: 0 }));
        assert!((0..8).all(|x| (0..8).all(|y| b.cell(Pos { x, y }) == Cell::Empty)));
    }

    #[test]
    fn a_chip_only_turns_where_there_is_room() {
        let chip = Kind::Chip(Chip::Cd4011);
        let mut b = Board::new(8, 3);
        let i = b.place(chip, Pos { x: 0, y: 0 }, Dir::W, false).unwrap();
        assert!(!b.turn(i, Dir::N), "seven tall won't fit in three");
        assert!(b.turn(i, Dir::E));
        assert_eq!(b.parts[i].pin_ports()[0], (Pos { x: 6, y: 0 }, Dir::N));
        b.place(Kind::Resistor { ohms: 1e3, watts: 0.25 }, Pos { x: 7, y: 2 }, Dir::W, false).unwrap();
        assert!(b.place(chip, Pos { x: 1, y: 1 }, Dir::W, false).is_none(), "overlaps");
    }

    #[test]
    fn copper_meets_a_chip_only_at_its_legs() {
        let mut b = Board::new(8, 4);
        b.place(Kind::Chip(Chip::Cd4011), Pos { x: 0, y: 1 }, Dir::W, false).unwrap();
        // Pin 14 faces north from the top left tile; pin 13 from the next. No legs on the ends.
        assert!(b.connect(Pos { x: 0, y: 0 }, Dir::S));
        assert!(!b.connect(Pos { x: 7, y: 1 }, Dir::W));
        let n = Netlist::build(&b);
        assert_eq!(n.pin_net[0].len(), 14);
        assert_eq!(n.net_at(&b, Pos { x: 0, y: 0 }, Dir::S), Some(n.pin_net[0][13]));
    }

    #[test]
    fn a_text_board_draws_a_chip_by_repeating_its_letter() {
        let b = Board::parse(&["IIIIIII", "IIIIIII"], &[('I', Kind::Chip(Chip::Cd4011), Dir::W)]).unwrap();
        assert_eq!(b.parts.len(), 1);
        assert_eq!(b.parts[0].label, "IC1");
        assert!(Board::parse(&["IIIIII", "IIIIII"], &[('I', Kind::Chip(Chip::Cd4011), Dir::W)]).is_err(), "too small");
    }

    #[test]
    fn shorted_supply_still_solves_in_current_limit() {
        let b = Board::parse(
            &["┌P┐", "└─┘"],
            &[('P', Kind::Supply { volts: 5.0, limit: 0.1 }, Dir::W)],
        )
        .unwrap();
        let mut live = Live::build(&b);
        live.sim.solve_dc().unwrap();
        let psu = live.reading(&b, 0);
        assert!((psu.amps - 0.1).abs() < 1e-9, "{psu:?}");
    }
}
