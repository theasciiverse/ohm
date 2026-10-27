//! The circuit solver: modified nodal analysis on a small dense matrix.
//!
//! No terminal and no clock in here. A `Circuit` is nodes and parts; `solve_dc` finds the steady
//! state with capacitors open, `step(dt)` moves time on by one backward-Euler step. Batteries and
//! the bench supply are voltage sources with a current unknown each (so an ideal source works
//! too); a battery's internal resistance sits on its own internal node. Junctions (LEDs, later
//! diodes) are solved by Newton iteration with SPICE's junction-voltage limiting. A tiny
//! conductance from every node to ground keeps floating islands solvable.
//!
//! Parts heat up gradually: each one has a `heat` that follows its load with a time constant, and
//! burns open when it passes its limit. `repair_all` puts every burnt part back.
//!
//! Strip & Rail (paddylopez, MIT) was the reference for the overall shape: the same dense
//! elimination, `pnjlim` and LED model (Shockley junction plus series resistance, with the
//! forward voltage given at 10 mA). Written fresh in Rust.

/// Thermal voltage at room temperature.
const VT: f64 = 0.025_85;
/// Conductance from every node to ground.
const GMIN: f64 = 1e-12;
/// A closed switch's contact resistance.
const R_CLOSED: f64 = 0.01;
/// Newton iterations before giving up.
const MAX_ITER: usize = 200;
/// The current an LED's `vf` is given at.
const LED_I_REF: f64 = 0.010;
/// A fuse's heat follows its load with this time constant, seconds, and it opens at this load
/// (the square of 1.6 times its rating).
const FUSE_TAU: f64 = 0.25;
const FUSE_LIMIT: f64 = 2.56;
/// An electrolytic's stress follows its load with this time constant, seconds. It vents at
/// a load of 1: past its surge voltage (1.15 times its rating), or reversed by more than 1.5 V,
/// both from Vishay's "Aluminum Capacitors: Introduction" (document 25001).
const CAP_TAU: f64 = 0.5;
const CAP_SURGE: f64 = 1.15;
const CAP_REVERSE: f64 = 1.5;
/// A relay, after Sanyou's SRD series (datasheet V3.2): it pulls in at 75 % of its rated coil
/// voltage and drops out at 5 % (the sheet's worst cases), operates in 8 ms and releases in
/// 5 ms (the sheet's maxima), and its coil takes 0.36 W at its rating and no more than 130 % of
/// its rated voltage. Past that the coil heats with a 2 s time constant (chosen) and burns open.
pub const RELAY_PULL: f64 = 0.75;
pub const RELAY_DROP: f64 = 0.05;
pub const RELAY_OPERATE: f64 = 0.008;
pub const RELAY_RELEASE: f64 = 0.005;
pub const RELAY_WATTS: f64 = 0.36;
pub const RELAY_MAX: f64 = 1.3;
const COIL_TAU: f64 = 2.0;
/// A CMOS gate's output, after TI's CD4011B datasheet (SCHS021D): typically 1 mA at 0.4 V from
/// a 5 V supply and 2.6 mA at 0.5 V from 10 V, sinking or sourcing, which is 400 and 190 ohms;
/// `GATE_OHM_VOLTS / supply` fits both. 100 mW is the sheet's limit for each output
/// transistor; past it the gate heats (0.5 s, chosen) and burns at 1.5 times (chosen). The
/// supply's absolute limits are -0.5 V and 20 V.
const GATE_OHM_VOLTS: f64 = 2000.0;
const GATE_WATTS: f64 = 0.1;
pub const CMOS_MAX_VOLTS: f64 = 20.0;
/// Each CMOS input's capacitance, farads (the sheet's typical 5 pF): with the solver's tiny
/// leak to ground, an input left unconnected keeps its last level for seconds, as a real one
/// does.
pub const CMOS_INPUT_FARADS: f64 = 5e-12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Node(usize);

pub const GROUND: Node = Node(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PartId(usize);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SolveError {
    /// The matrix has no unique solution, e.g. two ideal sources fighting in a loop.
    Singular,
    /// Newton iteration did not settle.
    NoConvergence,
}

/// An LED's electrical character.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Led {
    /// Forward voltage at 10 mA.
    pub vf: f64,
    /// Series resistance, ohms.
    pub rs: f64,
    /// Emission coefficient.
    pub n: f64,
    /// Rated current: full brightness, and the point where heat starts to build.
    pub i_max: f64,
    /// Current it burns out at if held there.
    pub i_burn: f64,
}

impl Led {
    // Forward voltages per colour as in Strip & Rail's data/parts/led.json.
    pub const RED: Led = Led::colour(2.0);
    pub const YELLOW: Led = Led::colour(2.05);
    pub const GREEN: Led = Led::colour(2.15);
    pub const BLUE: Led = Led::colour(3.0);

    const fn colour(vf: f64) -> Led {
        Led { vf, rs: 10.0, n: 2.0, i_max: 0.030, i_burn: 0.045 }
    }

    /// Saturation current that puts `vf` at 10 mA.
    fn is(&self) -> f64 {
        let vd = self.vf - LED_I_REF * self.rs;
        LED_I_REF / ((vd / (self.n * VT)).exp() - 1.0)
    }
}

/// An NPN transistor's electrical character, for the Ebers-Moll transport model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Npn {
    /// Transport saturation current.
    pub is: f64,
    /// Forward and reverse current gain.
    pub bf: f64,
    pub br: f64,
    /// Power it can turn into heat, watts, and the collector current it is rated for, amps.
    pub watts: f64,
    pub i_max: f64,
    /// Junction capacitances, base to emitter and base to collector, farads: they give a
    /// transistor flipping a circuit over (a blinker) the moment it takes, without which the
    /// flip would have to happen in no time at all.
    pub cbe: f64,
    pub cbc: f64,
}

impl Npn {
    /// BC547B: IS, BF and BR from NXP's SPICE model (BF 294, the datasheet's typical hFE 290
    /// at 2 mA); 625 mW, 100 mA and the capacitances (input 10 pF, output 1.7 pF, typical)
    /// from onsemi's BC546 to BC548 datasheet.
    pub const BC547B: Npn = Npn { is: 2.39e-14, bf: 294.3, br: 7.946, watts: 0.625, i_max: 0.1, cbe: 10e-12, cbc: 1.7e-12 };
}

impl Led {
    /// A rectifier diode, 1N4001: IS 14.11 nA, N 1.984, RS 33.9 mohm (the Motorola databook's
    /// SPICE model), which puts 0.69 V across it at 10 mA and 0.96 V at 1 A (Vishay's sheet:
    /// 1.1 V at most); rated 1 A, and it burns at 3 A held (chosen).
    pub const N4001: Led = Led { vf: 0.691, rs: 0.0339, n: 1.984, i_max: 1.0, i_burn: 3.0 };
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// `watts` is the power rating; infinite for parts that never burn (a lamp, a test load).
    Resistor { ohms: f64, watts: f64 },
    /// Positive terminal `a`. With internal resistance the source sits between `mid` and `b`.
    Battery { volts: f64, r_int: f64 },
    /// Bench supply: constant voltage until the current limit, then constant current.
    Supply { volts: f64, limit: f64 },
    Switch { closed: bool },
    /// A fuse: a small resistance that heats by the square of its current over its rating.
    Fuse { ohms: f64, amps: f64 },
    /// Anode `a`, cathode `b`, the junction between `mid` and `b`.
    Led(Led),
    /// `volts` is an electrolytic's rating, positive terminal `a`; infinite for a capacitor
    /// that has no polarity and never fails (a ceramic).
    Capacitor { farads: f64, volts: f64 },
    /// A relay's coil, rated `volts`: a resistance whose current works the `contact`, a switch
    /// part of its own, after the operate or release time. `nc`: the contact is closed at rest.
    Coil { ohms: f64, volts: f64, contact: usize, nc: bool },
    /// Collector `a`, emitter `b`, base `mid`.
    Npn(Npn),
    /// A CMOS NAND gate's output `a`, its supply's - at `b` and + at `mid`, reading the inputs
    /// on nodes `in_a` and `in_b`: switched to + or - through the gate's output resistance.
    /// It decides at the end of each step, so the next step sees it. `chip`: the first gate of
    /// its package, which burns as one.
    Nand { in_a: usize, in_b: usize, high: bool, chip: usize },
}

#[derive(Clone, Debug)]
struct Part {
    kind: Kind,
    a: usize,
    b: usize,
    /// Internal node (battery, LED), or 0 for none.
    mid: usize,
    /// Index of this part's current unknown among the branches, for sources.
    branch: Option<usize>,
    // State carried between solves.
    burnt: bool,
    heat: f64,
    /// Junction voltage at the last Newton iterate (LED; a transistor's base-emitter), and a
    /// transistor's base-collector.
    vd: f64,
    vd2: f64,
    /// Capacitor voltage after the last step; a transistor's base-emitter, and its
    /// base-collector in `cap_v2`.
    cap_v: f64,
    cap_v2: f64,
    /// Supply is in current limit; a relay is energised (its contact has moved).
    cc: bool,
    /// How long a relay's coil has wanted the other state, seconds.
    timer: f64,
    // Results of the last solve; `base` is a transistor's base current.
    current: f64,
    base: f64,
    power: f64,
}

/// What a solve treats capacitors as.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Dc,
    Step(f64),
}

pub struct Circuit {
    nodes: usize,
    branches: usize,
    parts: Vec<Part>,
    /// Node voltages (nodes 1..) then branch currents.
    x: Vec<f64>,
    /// Simulated time, seconds.
    pub time: f64,
    /// No step taken yet: the first starts with nanosecond steps (see `step`).
    fresh: bool,
}

impl Default for Circuit {
    fn default() -> Self {
        Self::new()
    }
}

impl Circuit {
    pub fn new() -> Self {
        Circuit { nodes: 1, branches: 0, parts: Vec::new(), x: Vec::new(), time: 0.0, fresh: true }
    }

    pub fn node(&mut self) -> Node {
        self.nodes += 1;
        Node(self.nodes - 1)
    }

    fn add(&mut self, kind: Kind, a: Node, b: Node, mid: bool, branch: bool) -> PartId {
        let mid = if mid { self.node().0 } else { 0 };
        let branch = branch.then(|| {
            self.branches += 1;
            self.branches - 1
        });
        self.parts.push(Part {
            kind,
            a: a.0,
            b: b.0,
            mid,
            branch,
            burnt: false,
            heat: 0.0,
            vd: 0.0,
            vd2: 0.0,
            cap_v: 0.0,
            cap_v2: 0.0,
            cc: false,
            timer: 0.0,
            current: 0.0,
            base: 0.0,
            power: 0.0,
        });
        PartId(self.parts.len() - 1)
    }

    /// A resistor that never burns (a lamp, a test load).
    pub fn resistor(&mut self, a: Node, b: Node, ohms: f64) -> PartId {
        self.resistor_rated(a, b, ohms, f64::INFINITY)
    }

    /// A resistor with a power rating in watts.
    pub fn resistor_rated(&mut self, a: Node, b: Node, ohms: f64, watts: f64) -> PartId {
        self.add(Kind::Resistor { ohms, watts }, a, b, false, false)
    }

    pub fn battery(&mut self, pos: Node, neg: Node, volts: f64, r_int: f64) -> PartId {
        self.add(Kind::Battery { volts, r_int }, pos, neg, r_int > 0.0, true)
    }

    pub fn supply(&mut self, pos: Node, neg: Node, volts: f64, limit: f64) -> PartId {
        self.add(Kind::Supply { volts, limit }, pos, neg, false, true)
    }

    /// A fast-acting fuse rated `amps`, with its cold resistance `ohms`.
    pub fn fuse(&mut self, a: Node, b: Node, ohms: f64, amps: f64) -> PartId {
        self.add(Kind::Fuse { ohms, amps }, a, b, false, false)
    }

    pub fn switch(&mut self, a: Node, b: Node, closed: bool) -> PartId {
        self.add(Kind::Switch { closed }, a, b, false, false)
    }

    pub fn led(&mut self, anode: Node, cathode: Node, led: Led) -> PartId {
        self.add(Kind::Led(led), anode, cathode, true, false)
    }

    /// A capacitor that never fails (a ceramic).
    pub fn capacitor(&mut self, a: Node, b: Node, farads: f64) -> PartId {
        self.electrolytic(a, b, farads, f64::INFINITY)
    }

    /// An electrolytic rated `volts`, positive terminal `a`: it vents (open) when held past
    /// its surge voltage or reversed.
    pub fn electrolytic(&mut self, pos: Node, neg: Node, farads: f64, volts: f64) -> PartId {
        self.add(Kind::Capacitor { farads, volts }, pos, neg, false, false)
    }

    /// An NPN transistor: collector, base, emitter.
    ///
    /// No two transistors are quite alike: each one added has a saturation current 1 % above
    /// the last (chosen; real spread is far wider), so a symmetric circuit has a side to tip to.
    pub fn npn(&mut self, c: Node, base: Node, e: Node, model: Npn) -> PartId {
        let k = self.parts.iter().filter(|p| matches!(p.kind, Kind::Npn(_))).count();
        let model = Npn { is: model.is * (1.0 + 0.01 * k as f64), ..model };
        let id = self.add(Kind::Npn(model), c, e, false, false);
        self.parts[id.0].mid = base.0;
        id
    }

    /// One gate of a CMOS NAND package: output, two inputs, the supply's + and -. `chip` is
    /// the package's first gate (or `None` for this one), so the package burns as one.
    pub fn nand(&mut self, out: Node, in_a: Node, in_b: Node, vdd: Node, vss: Node, chip: Option<PartId>) -> PartId {
        let k = self.parts.len();
        let chip = chip.map_or(k, |c| c.0);
        let id = self.add(Kind::Nand { in_a: in_a.0, in_b: in_b.0, high: true, chip }, out, vss, false, false);
        self.parts[id.0].mid = vdd.0;
        id
    }

    /// A gate's output is high.
    pub fn gate_high(&self, id: PartId) -> bool {
        matches!(self.parts[id.0].kind, Kind::Nand { high: true, .. })
    }

    pub fn set_gate_high(&mut self, id: PartId, on: bool) {
        if let Kind::Nand { high, .. } = &mut self.parts[id.0].kind {
            *high = on;
        }
    }

    /// A transistor's base current, into the base.
    pub fn base_current(&self, id: PartId) -> f64 {
        self.parts[id.0].base
    }

    /// A relay rated `volts`: its coil between `coil_a` and `coil_b` (resistance from the
    /// coil's power, as on the SRD sheet: 400 Ω at 12 V), its contact between `sw_a` and `sw_b`,
    /// open at rest, or closed at rest if `nc`. Returns the coil.
    pub fn relay(&mut self, coil_a: Node, coil_b: Node, sw_a: Node, sw_b: Node, volts: f64, nc: bool) -> PartId {
        let contact = self.switch(sw_a, sw_b, nc).0;
        let ohms = volts * volts / RELAY_WATTS;
        self.add(Kind::Coil { ohms, volts, contact, nc }, coil_a, coil_b, false, false)
    }

    /// A relay's contact, as a part of its own.
    pub fn contact(&self, coil: PartId) -> Option<PartId> {
        match self.parts[coil.0].kind {
            Kind::Coil { contact, .. } => Some(PartId(contact)),
            _ => None,
        }
    }

    /// The relay has pulled in: its contact has moved away from rest.
    pub fn energised(&self, coil: PartId) -> bool {
        self.parts[coil.0].cc
    }

    /// Puts a relay pulled in or at rest at once, as if it had been so beforehand.
    pub fn set_energised(&mut self, coil: PartId, on: bool) {
        if let Kind::Coil { contact, nc, .. } = self.parts[coil.0].kind {
            self.parts[coil.0].cc = on;
            self.parts[coil.0].timer = 0.0;
            self.set_switch(PartId(contact), on != nc);
        }
    }

    /// A capacitor's voltage, positive terminal over negative.
    pub fn cap_voltage(&self, id: PartId) -> f64 {
        self.parts[id.0].cap_v
    }

    /// Sets a capacitor's voltage, as if charged that far beforehand.
    pub fn set_cap_voltage(&mut self, id: PartId, v: f64) {
        self.parts[id.0].cap_v = v;
    }

    pub fn set_switch(&mut self, id: PartId, closed: bool) {
        if let Kind::Switch { closed: c } = &mut self.parts[id.0].kind {
            *c = closed;
        }
    }

    /// Marks a part burnt open (carried over when a board is rebuilt), or mends it, cold.
    pub fn set_burnt(&mut self, id: PartId, burnt: bool) {
        let p = &mut self.parts[id.0];
        p.burnt = burnt;
        if !burnt {
            p.heat = 0.0;
        }
    }

    /// Puts every burnt part back, cold.
    pub fn repair_all(&mut self) {
        for p in &mut self.parts {
            p.burnt = false;
            p.heat = 0.0;
        }
    }

    pub fn voltage(&self, n: Node) -> f64 {
        if n.0 == 0 { 0.0 } else { self.x.get(n.0 - 1).copied().unwrap_or(0.0) }
    }

    /// Current through the part from `a` to `b`. For a battery or supply: the current it
    /// delivers out of its positive terminal.
    pub fn current(&self, id: PartId) -> f64 {
        self.parts[id.0].current
    }

    /// Power the part turns into heat or light. For a source: the power it delivers.
    pub fn power(&self, id: PartId) -> f64 {
        self.parts[id.0].power
    }

    /// Load relative to the rating: 1.0 is running at the rating; the part burns at its limit.
    pub fn heat(&self, id: PartId) -> f64 {
        self.parts[id.0].heat
    }

    pub fn burnt(&self, id: PartId) -> bool {
        self.parts[id.0].burnt
    }

    /// The supply is holding its current limit.
    pub fn limiting(&self, id: PartId) -> bool {
        self.parts[id.0].cc
    }

    /// Steady state with every capacitor open.
    pub fn solve_dc(&mut self) -> Result<(), SolveError> {
        self.solve(Mode::Dc)
    }

    /// Moves time on by `dt` seconds: one backward-Euler step, then heat and burn-out.
    ///
    /// The first step after a circuit is built starts at a nanosecond and grows by a tenth each
    /// time until it has covered `dt` (about 150 small steps). Backward Euler damps anything
    /// faster than its step, which would hold a flip-flop balanced half on, half off at
    /// power-up; at nanoseconds it tips over one way, as a real one does.
    pub fn step(&mut self, dt: f64) -> Result<(), SolveError> {
        if !std::mem::take(&mut self.fresh) {
            return self.step_within(dt, 0);
        }
        let (mut done, mut h) = (0.0, 1e-9f64);
        while done < dt {
            let take = h.min(dt - done);
            self.step_within(take, 0)?;
            done += take;
            h *= 1.1;
        }
        Ok(())
    }

    /// A transistor's junction state (its two junction voltages, as Newton last left them and
    /// as its capacitances hold them), to carry over a rebuild.
    pub fn junctions(&self, id: PartId) -> [f64; 4] {
        let p = &self.parts[id.0];
        [p.vd, p.vd2, p.cap_v, p.cap_v2]
    }

    pub fn set_junctions(&mut self, id: PartId, j: [f64; 4]) {
        let p = &mut self.parts[id.0];
        [p.vd, p.vd2, p.cap_v, p.cap_v2] = j;
    }

    /// A step, split in halves when Newton can't settle it: where a circuit flips over (a
    /// transistor blinker), the jump is too big to take in one.
    fn step_within(&mut self, dt: f64, depth: u32) -> Result<(), SolveError> {
        let saved: Vec<(f64, f64)> = self.parts.iter().map(|p| (p.vd, p.vd2)).collect();
        let x = self.x.clone();
        match self.solve(Mode::Step(dt)) {
            Err(SolveError::NoConvergence) if depth < 24 => {
                self.x = x;
                for (p, (vd, vd2)) in self.parts.iter_mut().zip(saved) {
                    p.vd = vd;
                    p.vd2 = vd2;
                }
                self.step_within(dt / 2.0, depth + 1)?;
                return self.step_within(dt / 2.0, depth + 1);
            }
            r => r?,
        }
        if self.heat_up(dt) {
            // Something burnt open during this step: solve the step again without it.
            self.solve(Mode::Step(dt))?;
        }
        for p in &mut self.parts {
            if let Kind::Capacitor { .. } = p.kind {
                p.cap_v = vdiff(&self.x, p.a, p.b);
            }
            if let Kind::Npn(_) = p.kind {
                p.cap_v = vdiff(&self.x, p.mid, p.b);
                p.cap_v2 = vdiff(&self.x, p.mid, p.a);
            }
        }
        self.move_contacts(dt);
        self.switch_gates();
        self.time += dt;
        Ok(())
    }

    /// Each relay's coil pulls its contact over once it has carried enough current for the
    /// operate time, and lets go once it has carried too little for the release time. The
    /// contact moves at the end of the step, so the next step sees it.
    fn move_contacts(&mut self, dt: f64) {
        for k in 0..self.parts.len() {
            let p = &self.parts[k];
            let Kind::Coil { ohms, volts, contact, nc } = p.kind else { continue };
            // The magnet goes by the coil's current, so a burnt coil lets go.
            let v = p.current.abs() * ohms;
            let want = if p.cc { v >= RELAY_DROP * volts } else { v >= RELAY_PULL * volts };
            let p = &mut self.parts[k];
            if want == p.cc {
                p.timer = 0.0;
                continue;
            }
            p.timer += dt;
            let wait = if want { RELAY_OPERATE } else { RELAY_RELEASE };
            if p.timer >= wait - dt / 2.0 {
                p.cc = want;
                p.timer = 0.0;
                if let Kind::Switch { closed } = &mut self.parts[contact].kind {
                    *closed = want != nc;
                }
            }
        }
    }

    /// Every gate reads its inputs against half its supply and sets its output for the next
    /// step. The CD4011B has no hysteresis; its sheet guarantees low below 30 % of the supply
    /// and high above 70 %, and switches about halfway.
    fn switch_gates(&mut self) {
        let x = &self.x;
        for p in &mut self.parts {
            let supply = vdiff(x, p.mid, p.b);
            if let Kind::Nand { in_a, in_b, high, .. } = &mut p.kind {
                let a = vdiff(x, *in_a, p.b) > supply / 2.0;
                let b = vdiff(x, *in_b, p.b) > supply / 2.0;
                *high = !(a && b);
                p.cap_v = supply;
            }
        }
    }

    /// Heats every part toward its load; true if anything burnt open.
    fn heat_up(&mut self, dt: f64) -> bool {
        let mut any = false;
        let x = &self.x;
        for p in &mut self.parts {
            if p.burnt {
                continue;
            }
            // (load relative to rating, time constant in seconds, limit)
            let (load, tau, limit) = match p.kind {
                Kind::Resistor { watts, .. } if watts.is_finite() => (p.power / watts, 0.5, 2.0),
                Kind::Led(led) => (p.current.max(0.0) / led.i_max, 0.02, led.i_burn / led.i_max),
                // Heat goes as the square of the current. Fitted to IEC 60127-2's fast-acting
                // limits: it carries 1.5 times its rating for good (an hour or more there),
                // opens at 2.1 times (within 30 minutes there), at 2.75 times in 0.05 to 2 s,
                // at 4 times in 0.01 to 0.3 s and at 10 times within 20 ms.
                Kind::Fuse { amps, .. } => ((p.current / amps).powi(2), FUSE_TAU, FUSE_LIMIT),
                Kind::Capacitor { volts, .. } if volts.is_finite() => {
                    let v = vdiff(x, p.a, p.b);
                    ((v / (CAP_SURGE * volts)).max(-v / CAP_REVERSE).max(0.0).powi(2), CAP_TAU, 1.0)
                }
                // At its maximum allowable voltage a coil takes 1.69 times its rated power.
                Kind::Coil { .. } => (p.power / (RELAY_WATTS * RELAY_MAX * RELAY_MAX), COIL_TAU, 1.0),
                // Heat by power or by collector current, whichever is further past its rating;
                // like a resistor it burns at twice (chosen).
                Kind::Npn(q) => ((p.power / q.watts).max(p.current.abs() / q.i_max), 0.5, 2.0),
                // A gate by its output's power; a supply past the absolute limits, or
                // backwards, burns the package at once.
                Kind::Nand { .. } => {
                    let supply = vdiff(x, p.mid, p.b);
                    let abused = !(-0.5..=CMOS_MAX_VOLTS).contains(&supply);
                    (if abused { 100.0 } else { p.power / GATE_WATTS }, 0.5, 1.5)
                }
                _ => continue,
            };
            p.heat += (load - p.heat) * (1.0 - (-dt / tau).exp());
            if p.heat >= limit {
                p.burnt = true;
                any = true;
            }
        }
        // A package burns as one.
        let burnt_chips: Vec<usize> = self.parts.iter().filter_map(|p| match p.kind {
            Kind::Nand { chip, .. } if p.burnt => Some(chip),
            _ => None,
        }).collect();
        for p in &mut self.parts {
            if let Kind::Nand { chip, .. } = p.kind
                && !p.burnt
                && burnt_chips.contains(&chip)
            {
                p.burnt = true;
                any = true;
            }
        }
        any
    }

    fn solve(&mut self, mode: Mode) -> Result<(), SolveError> {
        // The supply's mode is decided outside Newton: solve, check, flip, solve again.
        for _ in 0..4 {
            self.newton(mode)?;
            let mut flipped = false;
            for p in &mut self.parts {
                if let Kind::Supply { volts, limit } = p.kind {
                    let delivered = -self.x[self.nodes - 1 + p.branch.unwrap()];
                    let v = vdiff(&self.x, p.a, p.b);
                    if !p.cc && delivered > limit {
                        p.cc = true;
                        flipped = true;
                    } else if p.cc && v > volts {
                        p.cc = false;
                        flipped = true;
                    }
                }
            }
            if !flipped {
                break;
            }
        }
        self.record(mode);
        Ok(())
    }

    fn newton(&mut self, mode: Mode) -> Result<(), SolveError> {
        let n = self.nodes - 1 + self.branches;
        if self.x.len() != n {
            self.x = vec![0.0; n];
        }
        for _ in 0..MAX_ITER {
            let mut m = Matrix::new(n);
            for k in 1..self.nodes {
                m.g(k, 0, GMIN);
            }
            for p in &self.parts {
                self.stamp(p, mode, &mut m);
            }
            let x = m.solve().ok_or(SolveError::Singular)?;

            let mut settled = x
                .iter()
                .zip(&self.x)
                .all(|(new, old)| (new - old).abs() <= 1e-6 + 1e-6 * new.abs());
            for p in &mut self.parts {
                if let Kind::Led(led) = p.kind {
                    let vt = led.n * VT;
                    let vcrit = vt * (vt / (std::f64::consts::SQRT_2 * led.is())).ln();
                    let raw = vdiff(&x, p.mid, p.b);
                    let limited = pnjlim(raw, p.vd, vt, vcrit);
                    if limited != raw || (limited - p.vd).abs() > 1e-7 {
                        settled = false;
                    }
                    p.vd = limited;
                }
                if let Kind::Npn(q) = p.kind {
                    let vcrit = VT * (VT / (std::f64::consts::SQRT_2 * q.is)).ln();
                    for (raw, old) in [(vdiff(&x, p.mid, p.b), &mut p.vd), (vdiff(&x, p.mid, p.a), &mut p.vd2)] {
                        let limited = pnjlim(raw, *old, VT, vcrit);
                        if limited != raw || (limited - *old).abs() > 1e-7 {
                            settled = false;
                        }
                        *old = limited;
                    }
                }
            }
            self.x = x;
            if settled {
                return Ok(());
            }
        }
        Err(SolveError::NoConvergence)
    }

    fn stamp(&self, p: &Part, mode: Mode, m: &mut Matrix) {
        if p.burnt {
            return;
        }
        match p.kind {
            Kind::Resistor { ohms, .. } | Kind::Fuse { ohms, .. } | Kind::Coil { ohms, .. } => m.g(p.a, p.b, 1.0 / ohms),
            Kind::Switch { closed } => {
                if closed {
                    m.g(p.a, p.b, 1.0 / R_CLOSED);
                }
            }
            Kind::Battery { volts, r_int } => {
                let br = self.nodes - 1 + p.branch.unwrap();
                if r_int > 0.0 {
                    m.g(p.a, p.mid, 1.0 / r_int);
                    m.vsource(p.mid, p.b, br, volts);
                } else {
                    m.vsource(p.a, p.b, br, volts);
                }
            }
            Kind::Supply { volts, limit } => {
                let br = self.nodes - 1 + p.branch.unwrap();
                if p.cc {
                    // The branch current is unused in current limit; pin it to zero.
                    m.set(br, br, 1.0);
                    m.isource(p.b, p.a, limit);
                } else {
                    m.vsource(p.a, p.b, br, volts);
                }
            }
            Kind::Led(led) => {
                m.g(p.a, p.mid, 1.0 / led.rs);
                let vt = led.n * VT;
                let is = led.is();
                let e = (p.vd / vt).exp();
                let id = is * (e - 1.0);
                let gd = is / vt * e + GMIN;
                m.g(p.mid, p.b, gd);
                m.isource(p.mid, p.b, id - gd * p.vd);
            }
            Kind::Npn(q) => {
                // Ebers-Moll transport model: a diode base to emitter, a diode base to
                // collector, and the transport current collector to emitter.
                let (c, b, e) = (p.a, p.mid, p.b);
                let (ef, er) = ((p.vd / VT).exp(), (p.vd2 / VT).exp());
                let (gf, gr) = (q.is / VT * ef, q.is / VT * er);
                let (ibe, gbe) = (q.is / q.bf * (ef - 1.0), gf / q.bf + GMIN);
                let (ibc, gbc) = (q.is / q.br * (er - 1.0), gr / q.br + GMIN);
                m.g(b, e, gbe);
                m.isource(b, e, ibe - gbe * p.vd);
                m.g(b, c, gbc);
                m.isource(b, c, ibc - gbc * p.vd2);
                let ict = q.is * (ef - er);
                // Linearised: ict + gf (vbe - vd) - gr (vbc - vd2), out of c into e.
                m.node(c, b, gf - gr);
                m.node(c, e, -gf);
                m.node(c, c, gr);
                m.node(e, b, -(gf - gr));
                m.node(e, e, gf);
                m.node(e, c, -gr);
                m.isource(c, e, ict - gf * p.vd + gr * p.vd2);
                if let Mode::Step(dt) = mode {
                    m.g(b, e, q.cbe / dt);
                    m.isource(e, b, q.cbe / dt * p.cap_v);
                    m.g(b, c, q.cbc / dt);
                    m.isource(c, b, q.cbc / dt * p.cap_v2);
                }
            }
            Kind::Nand { high, .. } => {
                let rail = if high { p.mid } else { p.b };
                m.g(p.a, rail, 1.0 / gate_ohms(p.cap_v));
            }
            Kind::Capacitor { farads, .. } => {
                if let Mode::Step(dt) = mode {
                    let g = farads / dt;
                    m.g(p.a, p.b, g);
                    m.isource(p.b, p.a, g * p.cap_v);
                }
            }
        }
    }

    /// Currents and powers from the solved voltages.
    fn record(&mut self, mode: Mode) {
        let x = &self.x;
        let base = self.nodes - 1;
        for p in &mut self.parts {
            let v = vdiff(x, p.a, p.b);
            let i = if p.burnt {
                0.0
            } else {
                match p.kind {
                    Kind::Resistor { ohms, .. } | Kind::Fuse { ohms, .. } | Kind::Coil { ohms, .. } => v / ohms,
                    Kind::Switch { closed } => if closed { v / R_CLOSED } else { 0.0 },
                    Kind::Battery { .. } => -x[base + p.branch.unwrap()],
                    Kind::Supply { limit, .. } => {
                        if p.cc { limit } else { -x[base + p.branch.unwrap()] }
                    }
                    Kind::Led(led) => (v - vdiff(x, p.mid, p.b)) / led.rs,
                    Kind::Npn(_) => 0.0,
                    // Out of the output pin: from + when high, back into - (negative) when low.
                    Kind::Nand { high, .. } => vdiff(x, if high { p.mid } else { p.b }, p.a) / gate_ohms(p.cap_v),
                    Kind::Capacitor { farads, .. } => match mode {
                        Mode::Step(dt) => farads / dt * (v - p.cap_v),
                        Mode::Dc => 0.0,
                    },
                }
            };
            if let (Kind::Npn(q), false) = (p.kind, p.burnt) {
                let (vbe, vbc) = (vdiff(x, p.mid, p.b), vdiff(x, p.mid, p.a));
                let (ef, er) = ((vbe / VT).exp(), (vbc / VT).exp());
                let ibe = q.is / q.bf * (ef - 1.0);
                let ibc = q.is / q.br * (er - 1.0);
                p.base = ibe + ibc;
                p.current = q.is * (ef - er) - ibc;
                p.power = v * p.current + vbe * p.base;
                continue;
            }
            p.base = 0.0;
            p.current = i;
            p.power = if let Kind::Nand { .. } = p.kind { i * i * gate_ohms(p.cap_v) } else { v * i };
        }
    }
}

/// A CMOS gate's output resistance on a supply of `volts` (taken from the last step, so it
/// doesn't move under Newton); below 3 V, the sheet's minimum, as at 3 V.
fn gate_ohms(volts: f64) -> f64 {
    GATE_OHM_VOLTS / volts.max(3.0)
}

/// Voltage of node `a` minus node `b` in a solution vector.
fn vdiff(x: &[f64], a: usize, b: usize) -> f64 {
    let v = |k: usize| if k == 0 { 0.0 } else { x[k - 1] };
    v(a) - v(b)
}

/// SPICE's junction-voltage limiting: keeps Newton from leaping up the exponential.
fn pnjlim(vnew: f64, vold: f64, vt: f64, vcrit: f64) -> f64 {
    if vnew > vcrit && (vnew - vold).abs() > 2.0 * vt {
        if vold > 0.0 {
            let arg = 1.0 + (vnew - vold) / vt;
            if arg > 0.0 { vold + vt * arg.ln() } else { vcrit }
        } else {
            vt * (vnew / vt).ln()
        }
    } else {
        vnew
    }
}

/// The MNA system: a dense square matrix and its right-hand side. Rows and columns are the
/// unknowns; node `k` is unknown `k - 1`, ground has none.
struct Matrix {
    n: usize,
    a: Vec<f64>,
    z: Vec<f64>,
}

impl Matrix {
    fn new(n: usize) -> Self {
        Matrix { n, a: vec![0.0; n * n], z: vec![0.0; n] }
    }

    fn set(&mut self, r: usize, c: usize, v: f64) {
        self.a[r * self.n + c] += v;
    }

    fn node(&mut self, r: usize, c: usize, v: f64) {
        if r != 0 && c != 0 {
            self.set(r - 1, c - 1, v);
        }
    }

    /// A conductance between nodes `a` and `b`.
    fn g(&mut self, a: usize, b: usize, g: f64) {
        self.node(a, a, g);
        self.node(b, b, g);
        self.node(a, b, -g);
        self.node(b, a, -g);
    }

    /// A current source pushing `i` amps from node `from`, through itself, into node `to`.
    fn isource(&mut self, from: usize, to: usize, i: f64) {
        if from != 0 {
            self.z[from - 1] -= i;
        }
        if to != 0 {
            self.z[to - 1] += i;
        }
    }

    /// A voltage source holding `pos` at `v` above `neg`; its current (into `pos`, through the
    /// source, out of `neg`) is unknown `br`.
    fn vsource(&mut self, pos: usize, neg: usize, br: usize, v: f64) {
        if pos != 0 {
            self.set(pos - 1, br, 1.0);
            self.set(br, pos - 1, 1.0);
        }
        if neg != 0 {
            self.set(neg - 1, br, -1.0);
            self.set(br, neg - 1, -1.0);
        }
        self.z[br] += v;
    }

    /// Gaussian elimination with partial pivoting.
    fn solve(mut self) -> Option<Vec<f64>> {
        let n = self.n;
        let a = &mut self.a;
        let z = &mut self.z;
        for k in 0..n {
            let mut p = k;
            let mut best = a[k * n + k].abs();
            for i in k + 1..n {
                let v = a[i * n + k].abs();
                if v > best {
                    best = v;
                    p = i;
                }
            }
            if best < 1e-18 {
                return None;
            }
            if p != k {
                for j in 0..n {
                    a.swap(k * n + j, p * n + j);
                }
                z.swap(k, p);
            }
            for i in k + 1..n {
                let f = a[i * n + k] / a[k * n + k];
                if f == 0.0 {
                    continue;
                }
                for j in k..n {
                    a[i * n + j] -= f * a[k * n + j];
                }
                z[i] -= f * z[k];
            }
        }
        let mut x = vec![0.0; n];
        for i in (0..n).rev() {
            let s: f64 = (i + 1..n).map(|j| a[i * n + j] * x[j]).sum();
            x[i] = (z[i] - s) / a[i * n + i];
        }
        Some(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(got: f64, want: f64, tol: f64) {
        assert!((got - want).abs() <= tol, "got {got}, want {want} +- {tol}");
    }

    #[test]
    fn divider_halves_the_voltage() {
        let mut c = Circuit::new();
        let top = c.node();
        let mid = c.node();
        c.battery(top, GROUND, 10.0, 0.0);
        c.resistor(top, mid, 1000.0);
        c.resistor(mid, GROUND, 1000.0);
        c.solve_dc().unwrap();
        close(c.voltage(mid), 5.0, 1e-6);
    }

    #[test]
    fn series_and_parallel_combine() {
        // 1k in series with two 2k in parallel: 2k in all, 6 V gives 3 mA, 3 V across the pair.
        let mut c = Circuit::new();
        let top = c.node();
        let mid = c.node();
        let bat = c.battery(top, GROUND, 6.0, 0.0);
        c.resistor(top, mid, 1000.0);
        let r2 = c.resistor(mid, GROUND, 2000.0);
        c.resistor(mid, GROUND, 2000.0);
        c.solve_dc().unwrap();
        close(c.current(bat), 0.003, 1e-9);
        close(c.voltage(mid), 3.0, 1e-6);
        close(c.current(r2), 0.0015, 1e-9);
        close(c.power(r2), 0.0045, 1e-9);
    }

    #[test]
    fn internal_resistance_sags_under_load() {
        // 9 V with 1 ohm inside, 8 ohm load: 1 A, 8 V at the terminals.
        let mut c = Circuit::new();
        let pos = c.node();
        let bat = c.battery(pos, GROUND, 9.0, 1.0);
        c.resistor(pos, GROUND, 8.0);
        c.solve_dc().unwrap();
        close(c.current(bat), 1.0, 1e-6);
        close(c.voltage(pos), 8.0, 1e-6);
        close(c.power(bat), 8.0, 1e-6);
    }

    #[test]
    fn a_short_is_limited_by_the_battery() {
        // 1.5 V, 0.2 ohm inside, shorted by a closed switch: 1.5 / 0.21 A.
        let mut c = Circuit::new();
        let pos = c.node();
        let bat = c.battery(pos, GROUND, 1.5, 0.2);
        let sw = c.switch(pos, GROUND, true);
        c.solve_dc().unwrap();
        close(c.current(bat), 1.5 / 0.21, 1e-6);
        c.set_switch(sw, false);
        c.solve_dc().unwrap();
        close(c.current(bat), 0.0, 1e-9);
        close(c.voltage(pos), 1.5, 1e-6);
    }

    #[test]
    fn led_with_resistor_draws_textbook_current() {
        // (5 V - 2.0 V) / 330 ohm = 9.1 mA; the LED's own drop is a little under 2.0 V there.
        let mut c = Circuit::new();
        let pos = c.node();
        let a = c.node();
        c.battery(pos, GROUND, 5.0, 0.0);
        c.resistor(pos, a, 330.0);
        let led = c.led(a, GROUND, Led::RED);
        c.solve_dc().unwrap();
        close(c.current(led), 0.0091, 0.0002);
        close(c.voltage(a), 2.0, 0.02);
    }

    #[test]
    fn led_forward_voltage_is_met_at_ten_milliamps() {
        // A bench supply in current limit at 10 mA drives the LED: its voltage is vf.
        for led in [Led::RED, Led::GREEN, Led::BLUE] {
            let mut c = Circuit::new();
            let pos = c.node();
            let s = c.supply(pos, GROUND, 12.0, 0.010);
            c.led(pos, GROUND, led);
            c.solve_dc().unwrap();
            assert!(c.limiting(s));
            close(c.voltage(pos), led.vf, 1e-3);
        }
    }

    #[test]
    fn backwards_led_stays_dark() {
        let mut c = Circuit::new();
        let pos = c.node();
        let a = c.node();
        c.battery(pos, GROUND, 5.0, 0.0);
        c.resistor(pos, a, 330.0);
        let led = c.led(GROUND, a, Led::RED);
        c.solve_dc().unwrap();
        assert!(c.current(led).abs() < 1e-6, "{}", c.current(led));
    }

    #[test]
    fn led_without_resistor_burns_out() {
        let mut c = Circuit::new();
        let pos = c.node();
        c.battery(pos, GROUND, 5.0, 0.0);
        let led = c.led(pos, GROUND, Led::RED);
        for _ in 0..100 {
            c.step(0.001).unwrap();
        }
        assert!(c.burnt(led));
        close(c.current(led), 0.0, 1e-9);
        c.repair_all();
        assert!(!c.burnt(led));
    }

    #[test]
    fn led_at_rated_current_survives() {
        let mut c = Circuit::new();
        let pos = c.node();
        let a = c.node();
        c.battery(pos, GROUND, 5.0, 0.0);
        c.resistor(pos, a, 100.0);
        let led = c.led(a, GROUND, Led::RED);
        for _ in 0..2000 {
            c.step(0.001).unwrap();
        }
        assert!(!c.burnt(led));
        assert!(c.heat(led) > 0.9 && c.heat(led) < 1.5, "{}", c.heat(led));
    }

    #[test]
    fn an_electrolytic_holds_its_surge_voltage_and_vents_past_it_or_reversed() {
        let run = |volts: f64, rating: f64| {
            let mut c = Circuit::new();
            let pos = c.node();
            let a = c.node();
            c.battery(pos, GROUND, volts, 0.0);
            c.resistor(pos, a, 10.0);
            let cap = c.electrolytic(a, GROUND, 100e-6, rating);
            for _ in 0..5000 {
                c.step(0.001).unwrap();
            }
            c.burnt(cap)
        };
        assert!(!run(12.0, 16.0));
        assert!(!run(10.0, 10.0));
        assert!(!run(11.0, 10.0), "within its surge voltage");
        assert!(run(12.0, 10.0));
        assert!(run(-4.5, 16.0), "reversed");
        assert!(!run(-1.0, 16.0), "a volt backwards is tolerated");
    }

    #[test]
    fn rc_charges_to_63_percent_in_one_tau() {
        // 1k and 1000 uF: tau = 1 s.
        let mut c = Circuit::new();
        let pos = c.node();
        let a = c.node();
        c.battery(pos, GROUND, 5.0, 0.0);
        c.resistor(pos, a, 1000.0);
        c.capacitor(a, GROUND, 1e-3);
        let dt = 0.0005;
        for _ in 0..2000 {
            c.step(dt).unwrap();
        }
        close(c.time, 1.0, 1e-9);
        close(c.voltage(a), 5.0 * (1.0 - (-1.0f64).exp()), 0.005);
    }

    #[test]
    fn rc_discharges_to_37_percent_in_one_tau() {
        let mut c = Circuit::new();
        let pos = c.node();
        let a = c.node();
        c.battery(pos, GROUND, 5.0, 0.0);
        let sw = c.switch(pos, a, true);
        c.resistor(a, GROUND, 1000.0);
        c.capacitor(a, GROUND, 1e-3);
        for _ in 0..100 {
            c.step(0.001).unwrap();
        }
        close(c.voltage(a), 5.0, 0.01);
        c.set_switch(sw, false);
        for _ in 0..2000 {
            c.step(0.0005).unwrap();
        }
        close(c.voltage(a), 5.0 * (-1.0f64).exp(), 0.005);
    }

    #[test]
    fn bench_supply_limits_current() {
        // 5 V, 100 mA limit, 10 ohm: wants 500 mA, gets 100 mA at 1 V.
        let mut c = Circuit::new();
        let pos = c.node();
        let s = c.supply(pos, GROUND, 5.0, 0.1);
        let load = c.resistor(pos, GROUND, 10.0);
        c.solve_dc().unwrap();
        assert!(c.limiting(s));
        close(c.current(load), 0.1, 1e-9);
        close(c.voltage(pos), 1.0, 1e-6);
    }

    #[test]
    fn bench_supply_leaves_limit_when_the_load_lightens() {
        let mut c = Circuit::new();
        let pos = c.node();
        let s = c.supply(pos, GROUND, 5.0, 0.1);
        let sw = c.switch(pos, GROUND, true);
        c.resistor(pos, GROUND, 1000.0);
        c.solve_dc().unwrap();
        assert!(c.limiting(s));
        c.set_switch(sw, false);
        c.solve_dc().unwrap();
        assert!(!c.limiting(s));
        close(c.voltage(pos), 5.0, 1e-6);
        close(c.current(s), 0.005, 1e-9);
    }

    #[test]
    fn overloaded_resistor_smokes_then_burns_open() {
        // 100 ohm, 1/4 W, at 9 V: 0.81 W, over three times the rating.
        let mut c = Circuit::new();
        let pos = c.node();
        c.battery(pos, GROUND, 9.0, 0.0);
        let r = c.resistor_rated(pos, GROUND, 100.0, 0.25);
        c.step(0.01).unwrap();
        assert!(!c.burnt(r));
        let mut t = 0.0;
        while !c.burnt(r) && t < 5.0 {
            c.step(0.01).unwrap();
            t += 0.01;
        }
        assert!(c.burnt(r));
        // From cold to 2x rating at 3.24x load, tau 0.5 s: about 0.48 s.
        close(t, 0.5 * (3.24f64 / 1.24).ln(), 0.03);
        close(c.current(r), 0.0, 1e-9);
    }

    #[test]
    fn resistor_at_its_rating_never_burns() {
        let mut c = Circuit::new();
        let pos = c.node();
        c.battery(pos, GROUND, 5.0, 0.0);
        let r = c.resistor_rated(pos, GROUND, 100.0, 0.25);
        for _ in 0..1000 {
            c.step(0.01).unwrap();
        }
        assert!(!c.burnt(r));
        close(c.heat(r), 1.0, 1e-3);
    }

    /// How long a 1 A fuse carries `times` its rating before it opens; `None` if it holds for a
    /// simulated minute.
    fn fuse_opens_after(times: f64) -> Option<f64> {
        let mut c = Circuit::new();
        let pos = c.node();
        let mid = c.node();
        c.battery(pos, GROUND, 10.0, 0.0);
        let f = c.fuse(pos, mid, 0.1, 1.0);
        c.resistor(mid, GROUND, 10.0 / times - 0.1);
        let dt = 0.0005;
        for k in 1..=(60.0 / dt) as usize {
            c.step(dt).unwrap();
            if c.burnt(f) {
                return Some(k as f64 * dt);
            }
        }
        None
    }

    #[test]
    fn a_fuse_keeps_iec_60127_fast_acting_times() {
        assert_eq!(fuse_opens_after(1.0), None);
        assert_eq!(fuse_opens_after(1.5), None);
        let t = |times| fuse_opens_after(times).unwrap_or(f64::INFINITY);
        assert!(t(2.1) < 30.0 * 60.0, "{}", t(2.1));
        assert!((0.05..=2.0).contains(&t(2.75)), "{}", t(2.75));
        assert!((0.01..=0.3).contains(&t(4.0)), "{}", t(4.0));
        assert!(t(10.0) <= 0.02, "{}", t(10.0));
    }

    #[test]
    fn floating_island_solves_to_zero() {
        let mut c = Circuit::new();
        let pos = c.node();
        c.battery(pos, GROUND, 5.0, 0.0);
        c.resistor(pos, GROUND, 100.0);
        let x = c.node();
        let y = c.node();
        c.resistor(x, y, 470.0);
        c.solve_dc().unwrap();
        close(c.voltage(x), 0.0, 1e-9);
        close(c.voltage(y), 0.0, 1e-9);
    }

    #[test]
    fn a_relay_pulls_in_after_its_operate_time_and_closes_its_contact() {
        let mut c = Circuit::new();
        let pos = c.node();
        let lamp = c.node();
        c.battery(pos, GROUND, 12.0, 0.0);
        let k = c.relay(pos, GROUND, pos, lamp, 12.0, false);
        let load = c.resistor(lamp, GROUND, 24.0);
        let mut pulled = None;
        for n in 1..=50 {
            c.step(0.001).unwrap();
            if pulled.is_none() && c.energised(k) {
                pulled = Some(n);
            }
        }
        assert_eq!(pulled, Some(8), "the SRD operates within 8 ms");
        close(c.current(k), 0.030, 1e-4);
        close(c.current(load), 0.5, 1e-3);
    }

    #[test]
    fn a_relay_holds_between_its_drop_out_and_pull_in_voltages() {
        // 400 Ω coil behind 400 Ω: 6 V, half its rating.
        let run = |start_on: bool| {
            let mut c = Circuit::new();
            let pos = c.node();
            let a = c.node();
            let out = c.node();
            c.battery(pos, GROUND, 12.0, 0.0);
            c.resistor(pos, a, 400.0);
            let k = c.relay(a, GROUND, pos, out, 12.0, false);
            c.resistor(out, GROUND, 1000.0);
            c.set_energised(k, start_on);
            for _ in 0..200 {
                c.step(0.001).unwrap();
            }
            c.energised(k)
        };
        assert!(!run(false), "6 V can't pull a 12 V relay in");
        assert!(run(true), "but it holds one that is already in");
    }

    #[test]
    fn a_relay_through_its_own_closed_contact_buzzes() {
        let mut c = Circuit::new();
        let pos = c.node();
        let a = c.node();
        c.battery(pos, GROUND, 12.0, 0.0);
        let k = c.relay(a, GROUND, pos, a, 12.0, true);
        let mut flips = 0;
        let mut was = false;
        for _ in 0..1000 {
            c.step(0.001).unwrap();
            if c.energised(k) != was {
                flips += 1;
                was = !was;
            }
        }
        let hz = flips as f64 / 2.0;
        assert!((60.0..90.0).contains(&hz), "{hz} Hz");
    }

    #[test]
    fn a_capacitor_across_the_coil_holds_a_relay_in_for_r_c_ln_20() {
        let mut c = Circuit::new();
        let pos = c.node();
        let a = c.node();
        let out = c.node();
        c.battery(pos, GROUND, 12.0, 0.0);
        let sw = c.switch(pos, a, true);
        let k = c.relay(a, GROUND, pos, out, 12.0, false);
        c.resistor(out, GROUND, 1000.0);
        c.electrolytic(a, GROUND, 1000e-6, 25.0);
        for _ in 0..3000 {
            c.step(0.001).unwrap();
        }
        assert!(c.energised(k));
        c.set_switch(sw, false);
        let mut held = 0.0;
        while c.energised(k) && held < 5.0 {
            c.step(0.001).unwrap();
            held += 0.001;
        }
        // 400 Ω and 1000 µF from 12 V down to the 0.6 V drop-out: 0.4 s ln 20 = 1.20 s.
        close(held, 0.4 * 20f64.ln(), 0.02);
    }

    #[test]
    fn a_coil_takes_130_percent_but_burns_past_it() {
        let run = |volts: f64| {
            let mut c = Circuit::new();
            let pos = c.node();
            c.battery(pos, GROUND, volts, 0.0);
            let k = c.relay(pos, GROUND, pos, GROUND, 12.0, false);
            for _ in 0..20000 {
                c.step(0.001).unwrap();
            }
            c.burnt(k)
        };
        assert!(!run(15.0));
        assert!(run(18.0));
    }

    #[test]
    fn a_1n4001_drops_about_0_7_v_and_blocks_backwards() {
        let run = |volts: f64| {
            let mut c = Circuit::new();
            let pos = c.node();
            let a = c.node();
            c.battery(pos, GROUND, volts, 0.0);
            c.resistor(pos, a, 470.0);
            let d = c.led(a, GROUND, Led::N4001);
            c.solve_dc().unwrap();
            (c.voltage(a), c.current(d))
        };
        let (v, i) = run(9.0);
        assert!((0.68..0.76).contains(&v), "{v}");
        close(i, (9.0 - v) / 470.0, 1e-6);
        assert!(run(-9.0).1.abs() < 1e-7);
    }

    #[test]
    fn a_bc547b_amplifies_its_base_current_about_290_times() {
        let mut c = Circuit::new();
        let vcc = c.node();
        let b = c.node();
        let col = c.node();
        c.battery(vcc, GROUND, 9.0, 0.0);
        c.resistor(vcc, b, 1e6);
        c.resistor(vcc, col, 1000.0);
        let q = c.npn(col, b, GROUND, Npn::BC547B);
        c.solve_dc().unwrap();
        let beta = c.current(q) / c.base_current(q);
        assert!((beta - 294.0).abs() < 5.0, "{beta}");
        let vbe = c.voltage(b);
        assert!((0.55..0.7).contains(&vbe), "VBE {vbe}");
    }

    #[test]
    fn a_bc547b_switched_hard_on_saturates() {
        // 10 mA collector, 0.85 mA base: the datasheet gives 0.09 V typical, 0.25 V at most, at
        // 10 mA with 0.5 mA in.
        let mut c = Circuit::new();
        let vcc = c.node();
        let b = c.node();
        let col = c.node();
        c.battery(vcc, GROUND, 9.0, 0.0);
        c.resistor(vcc, b, 10e3);
        c.resistor(vcc, col, 900.0);
        let q = c.npn(col, b, GROUND, Npn::BC547B);
        c.solve_dc().unwrap();
        let vce = c.voltage(col);
        assert!(vce > 0.0 && vce < 0.25, "VCE(sat) {vce}");
        close(c.current(q), (9.0 - vce) / 900.0, 1e-6);
    }

    #[test]
    fn two_transistors_and_two_capacitors_blink_at_1_over_1_4_r_c() {
        // The astable multivibrator: each side's collector through a capacitor to the other's
        // base. Each half lasts about ln 2 x R C, 0.69 x 47k x 10 uF = 0.33 s.
        let mut c = Circuit::new();
        let vcc = c.node();
        let (c1, c2, b1, b2) = (c.node(), c.node(), c.node(), c.node());
        c.battery(vcc, GROUND, 9.0, 0.0);
        c.resistor(vcc, c1, 1000.0);
        c.resistor(vcc, c2, 1000.0);
        c.resistor(vcc, b1, 47e3);
        c.resistor(vcc, b2, 47e3);
        c.capacitor(c1, b2, 10e-6);
        c.capacitor(c2, b1, 10e-6);
        let q1 = c.npn(c1, b1, GROUND, Npn::BC547B);
        c.npn(c2, b2, GROUND, Npn::BC547B);
        let mut rises = Vec::new();
        let mut was = false;
        for n in 0..5000 {
            c.step(0.001).unwrap();
            let on = c.current(q1) > 0.004;
            if on && !was && n > 500 {
                rises.push(n as f64 * 0.001);
            }
            was = on;
        }
        assert!(rises.len() >= 3, "{rises:?}");
        let period = (rises[rises.len() - 1] - rises[0]) / (rises.len() - 1) as f64;
        close(period, 2.0 * 2f64.ln() * 47e3 * 10e-6, 0.08);
    }

    #[test]
    fn a_flip_flop_comes_up_one_side_on_not_balanced() {
        // Two BC547s, each base fed through 10k from the other's collector, 470 in each
        // collector, an LED in each emitter. Balanced half on, half off is a solution, but an
        // unstable one: it must tip over.
        let mut c = Circuit::new();
        let vcc = c.node();
        let (c1, c2, b1, b2, e1, e2) = (c.node(), c.node(), c.node(), c.node(), c.node(), c.node());
        c.battery(vcc, GROUND, 9.0, 1.5);
        c.resistor(vcc, c1, 470.0);
        c.resistor(vcc, c2, 470.0);
        c.resistor(c1, b2, 10e3);
        c.resistor(c2, b1, 10e3);
        c.npn(c1, b1, e1, Npn::BC547B);
        c.npn(c2, b2, e2, Npn::BC547B);
        let l1 = c.led(e1, GROUND, Led::RED);
        let l2 = c.led(e2, GROUND, Led::RED);
        for _ in 0..100 {
            c.step(0.001).unwrap();
        }
        let (i1, i2) = (c.current(l1), c.current(l2));
        assert!(i1.min(i2) < 1e-4 && i1.max(i2) > 0.010, "{i1} {i2}");
    }

    /// A NAND gate on a 9 V battery with each input tied to + or -, its output into `load` ohms
    /// to -. Returns the output voltage after a few steps, and the gate.
    fn nand_out(a: bool, b: bool, load: f64) -> (f64, Circuit, PartId) {
        let mut c = Circuit::new();
        let (vdd, out) = (c.node(), c.node());
        c.battery(vdd, GROUND, 9.0, 0.0);
        let pick = |on: bool| if on { vdd } else { GROUND };
        let g = c.nand(out, pick(a), pick(b), vdd, GROUND, None);
        c.resistor(out, GROUND, load);
        for _ in 0..5 {
            c.step(0.001).unwrap();
        }
        (c.voltage(out), c, g)
    }

    #[test]
    fn a_nand_gate_is_low_only_with_both_inputs_high() {
        for (a, b) in [(false, false), (false, true), (true, false), (true, true)] {
            let (v, ..) = nand_out(a, b, 1e6);
            let want = if a && b { 0.0 } else { 9.0 };
            close(v, want, 0.01);
        }
    }

    #[test]
    fn a_cmos_output_has_the_datasheets_resistance() {
        // CD4011B, typical: 2.6 mA at 0.5 V below a 10 V supply, about 190 ohms. On 9 V, 222.
        let (v, c, g) = nand_out(false, false, 1000.0);
        close(v, 9.0 * 1000.0 / 1222.2, 0.01);
        close(c.current(g), v / 1000.0, 1e-6);
    }

    #[test]
    fn a_shorted_cmos_output_burns_but_a_led_load_does_not() {
        // A short on 9 V puts 365 mW in the output, past the sheet's 100 mW.
        let (_, mut c, g) = nand_out(false, false, 0.01);
        for _ in 0..2000 {
            c.step(0.001).unwrap();
        }
        assert!(c.burnt(g));
        let (_, mut c, g) = nand_out(false, false, 470.0);
        for _ in 0..2000 {
            c.step(0.001).unwrap();
        }
        assert!(!c.burnt(g), "{}", c.heat(g));
    }

    #[test]
    fn a_cmos_chip_powered_backwards_burns_whole() {
        let mut c = Circuit::new();
        let (vdd, out) = (c.node(), c.node());
        c.battery(GROUND, vdd, 9.0, 0.0);
        let first = c.nand(out, GROUND, GROUND, vdd, GROUND, None);
        let second = c.nand(out, GROUND, GROUND, vdd, GROUND, Some(first));
        for _ in 0..200 {
            c.step(0.001).unwrap();
        }
        assert!(c.burnt(first) && c.burnt(second));
    }

    #[test]
    fn a_floating_cmos_input_keeps_its_level_for_seconds() {
        // Input A held at + through a switch, input B at +; open the switch and A, with only
        // its 5 pF and the tiny leak, stays high a while: the output stays low.
        let mut c = Circuit::new();
        let (vdd, out, a) = (c.node(), c.node(), c.node());
        c.battery(vdd, GROUND, 9.0, 0.0);
        let sw = c.switch(vdd, a, true);
        c.capacitor(a, GROUND, CMOS_INPUT_FARADS);
        let g = c.nand(out, a, vdd, vdd, GROUND, None);
        c.resistor(out, GROUND, 1e6);
        for _ in 0..10 {
            c.step(0.001).unwrap();
        }
        assert!(!c.gate_high(g));
        c.set_switch(sw, false);
        for _ in 0..1000 {
            c.step(0.001).unwrap();
        }
        assert!(!c.gate_high(g), "still low a second later: {} V", c.voltage(a));
    }

    #[test]
    fn two_nand_gates_cross_coupled_remember() {
        // An S-R latch: each output into one input of the other gate, the free inputs held
        // high by 100k, a switch pulling one low sets or resets it.
        let mut c = Circuit::new();
        let vdd = c.node();
        let (q, nq, s, r) = (c.node(), c.node(), c.node(), c.node());
        c.battery(vdd, GROUND, 9.0, 0.0);
        c.resistor(vdd, s, 100e3);
        c.resistor(vdd, r, 100e3);
        let set = c.switch(s, GROUND, false);
        let reset = c.switch(r, GROUND, false);
        let a = c.nand(q, s, nq, vdd, GROUND, None);
        c.nand(nq, r, q, vdd, GROUND, Some(a));
        let run = |c: &mut Circuit| {
            for _ in 0..20 {
                c.step(0.001).unwrap();
            }
        };
        run(&mut c);
        c.set_switch(set, true);
        run(&mut c);
        c.set_switch(set, false);
        run(&mut c);
        assert!(c.voltage(q) > 8.9 && c.voltage(nq) < 0.1, "set and held");
        c.set_switch(reset, true);
        run(&mut c);
        c.set_switch(reset, false);
        run(&mut c);
        assert!(c.voltage(q) < 0.1 && c.voltage(nq) > 8.9, "reset and held");
    }

    #[test]
    fn two_ideal_sources_fighting_are_reported() {
        let mut c = Circuit::new();
        let pos = c.node();
        c.battery(pos, GROUND, 5.0, 0.0);
        c.battery(pos, GROUND, 3.0, 0.0);
        assert_eq!(c.solve_dc(), Err(SolveError::Singular));
    }
}
