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
    Capacitor { farads: f64 },
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
    /// Junction voltage at the last Newton iterate (LED).
    vd: f64,
    /// Capacitor voltage after the last step.
    cap_v: f64,
    /// Supply is in current limit.
    cc: bool,
    // Results of the last solve.
    current: f64,
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
}

impl Default for Circuit {
    fn default() -> Self {
        Self::new()
    }
}

impl Circuit {
    pub fn new() -> Self {
        Circuit { nodes: 1, branches: 0, parts: Vec::new(), x: Vec::new(), time: 0.0 }
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
            cap_v: 0.0,
            cc: false,
            current: 0.0,
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

    pub fn capacitor(&mut self, a: Node, b: Node, farads: f64) -> PartId {
        self.add(Kind::Capacitor { farads }, a, b, false, false)
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
    pub fn step(&mut self, dt: f64) -> Result<(), SolveError> {
        self.solve(Mode::Step(dt))?;
        if self.heat_up(dt) {
            // Something burnt open during this step: solve the step again without it.
            self.solve(Mode::Step(dt))?;
        }
        for p in &mut self.parts {
            if let Kind::Capacitor { .. } = p.kind {
                p.cap_v = vdiff(&self.x, p.a, p.b);
            }
        }
        self.time += dt;
        Ok(())
    }

    /// Heats every part toward its load; true if anything burnt open.
    fn heat_up(&mut self, dt: f64) -> bool {
        let mut any = false;
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
                _ => continue,
            };
            p.heat += (load - p.heat) * (1.0 - (-dt / tau).exp());
            if p.heat >= limit {
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
            Kind::Resistor { ohms, .. } | Kind::Fuse { ohms, .. } => m.g(p.a, p.b, 1.0 / ohms),
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
            Kind::Capacitor { farads } => {
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
                    Kind::Resistor { ohms, .. } | Kind::Fuse { ohms, .. } => v / ohms,
                    Kind::Switch { closed } => if closed { v / R_CLOSED } else { 0.0 },
                    Kind::Battery { .. } => -x[base + p.branch.unwrap()],
                    Kind::Supply { limit, .. } => {
                        if p.cc { limit } else { -x[base + p.branch.unwrap()] }
                    }
                    Kind::Led(led) => (v - vdiff(x, p.mid, p.b)) / led.rs,
                    Kind::Capacitor { farads } => match mode {
                        Mode::Step(dt) => farads / dt * (v - p.cap_v),
                        Mode::Dc => 0.0,
                    },
                }
            };
            p.current = i;
            p.power = v * i;
        }
    }
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
    fn two_ideal_sources_fighting_are_reported() {
        let mut c = Circuit::new();
        let pos = c.node();
        c.battery(pos, GROUND, 5.0, 0.0);
        c.battery(pos, GROUND, 3.0, 0.0);
        assert_eq!(c.solve_dc(), Err(SolveError::Singular));
    }
}
