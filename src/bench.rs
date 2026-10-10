//! The test bench: a job's spec cases, run against a board, and the diagnosis hints that say
//! what is wrong when a build doesn't work.
//!
//! Both ideas come from Strip & Rail (paddylopez, MIT): goals that judge what the circuit does,
//! so any working build passes; and hints ordered most fundamental first, each in three steps (a
//! nudge, the specific problem, the fix), built from the board's rules plus one powered trial so
//! they describe what the build actually does.

use crate::circuit::{Board, Kind, Live, Pos};

/// One simulation step for bench runs, seconds.
const DT: f64 = 0.001;
/// How long a case runs before its steady readings are taken, seconds.
const SETTLE: f64 = 0.5;
/// LED current below which it counts as dark.
const UNLIT: f64 = 0.001;

/// What a case checks.
#[derive(Clone, Debug, PartialEq)]
pub enum Check {
    /// The current through an LED or a lamp is in range.
    Lit { part: String, min: f64, max: f64 },
    /// An LED or a lamp stays dark.
    Dark { part: String },
    /// A test point's voltage is in range.
    Volts { point: String, min: f64, max: f64 },
    /// A fuse has blown by the end of the case: the fault it is there for was cleared.
    Blown { part: String },
    /// After the case's last event, an LED or a lamp stays lit at least `min` seconds and goes
    /// out within `max`.
    Lasts { part: String, min: f64, max: f64 },
    /// After the case's last event (or from the start), a test point's voltage gets to `volts`
    /// in `min` to `max` seconds, rising or falling to it.
    Reaches { point: String, volts: f64, min: f64, max: f64 },
    /// After the case's last event, an LED or a lamp blinks, or a relay chatters, `min` to
    /// `max` times a second, timed over `BLINK_WATCH` seconds once it has had `BLINK_START`
    /// to get going.
    Blinks { part: String, min: f64, max: f64 },
}

/// A blink check lets the circuit get going this long, then times it for this long, seconds.
const BLINK_START: f64 = 0.5;
const BLINK_WATCH: f64 = 6.0;

/// One condition the build must meet: how the bench sets the board up, and what it checks.
#[derive(Clone, Debug, PartialEq)]
pub struct Case {
    pub name: String,
    /// Every battery set to this voltage (a fresh cell, a tired one).
    pub battery: Option<f64>,
    /// Parts by label and what they are set to: a switch or button 1 (closed, held) or 0, an
    /// LDR's lux, a thermistor's °C, a pot's turn from 0 to 1.
    pub set: Vec<(String, f64)>,
    /// Parts set again partway through, as `set` sets them: (seconds, label, value), in order.
    pub events: Vec<(f64, String, f64)>,
    /// When a steady check takes its reading, seconds; by default `SETTLE` after the last event.
    pub at: Option<f64>,
    pub check: Check,
}

impl Case {
    /// A case with no events, read once it has settled.
    pub fn steady(name: &str, battery: Option<f64>, set: Vec<(String, f64)>, check: Check) -> Case {
        Case { name: name.into(), battery, set, events: Vec::new(), at: None, check }
    }

    /// When the last event happens: what timed checks count from.
    fn start(&self) -> f64 {
        self.events.iter().map(|e| e.0).fold(0.0, f64::max)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub pass: bool,
    /// The number the check measured, for the bench table.
    pub measured: Option<f64>,
    /// One plain sentence.
    pub why: String,
}

impl Outcome {
    fn fail(why: String) -> Self {
        Outcome { pass: false, measured: None, why }
    }
}

pub fn run_case(board: &Board, case: &Case) -> Outcome {
    let labels = case.set.iter().map(|(l, _)| l).chain(case.events.iter().map(|(_, l, _)| l));
    if let Some(label) = labels.into_iter().find(|l| board.part(l).is_none()) {
        let what = match label.trim_end_matches(|c: char| c.is_ascii_digit()) {
            "PB" => "a push button",
            "RV" => "a potentiometer",
            "LDR" => "a light sensor",
            "TH" => "a thermistor",
            _ => "a switch",
        };
        return Outcome::fail(format!("{label} isn't on the board: this job needs {what}."));
    }
    let mut b = with_settings(board, case);
    // A check can name several parts joined with `+` (`LED1+LED2`): each must pass.
    let names = match &case.check {
        Check::Lit { part, .. } | Check::Dark { part } | Check::Blown { part } | Check::Lasts { part, .. } | Check::Blinks { part, .. } => part,
        Check::Volts { point, .. } | Check::Reaches { point, .. } => point,
    };
    let mut targets = Vec::new();
    for name in names.split('+') {
        match b.part(name) {
            Some(i) => targets.push(i),
            None => return Outcome::fail(format!("{name} isn't on the board.")),
        }
    }
    let mut live = Live::build(&b);
    let start = case.start();
    let end = match &case.check {
        Check::Lasts { max, .. } | Check::Reaches { max, .. } => start + max + 0.5,
        Check::Blinks { .. } => start + BLINK_START + BLINK_WATCH,
        _ => case.at.unwrap_or(start + SETTLE),
    };
    // For a blink check: whether the part was on at the last step, and when it came on.
    let mut was_on: Option<bool> = None;
    let mut rises: Vec<f64> = Vec::new();
    let mut next = 0;
    // For timed checks: the reading where the count starts, and when it ended, from `start`.
    let mut from: Option<f64> = None;
    let mut ended: Option<f64> = None;
    let steps = (end / DT).round() as usize;
    for n in 0..steps {
        let t = n as f64 * DT;
        while let Some((when, label, v)) = case.events.get(next)
            && *when <= t + DT / 2.0
        {
            next += 1;
            let i = b.part(label).unwrap();
            b.parts[i].kind = set_kind(b.parts[i].kind, *v);
            match b.parts[i].kind {
                Kind::Switch { closed: on } => live.set_switch(i, on),
                Kind::Button { held, nc } => live.set_switch(i, held != nc),
                _ => {
                    let old = std::mem::replace(&mut live, Live::build(&b));
                    live.keep_charge(&old);
                }
            }
        }
        if live.sim.step(DT).is_err() {
            return Outcome::fail("The circuit can't be solved: two sources are fighting.".into());
        }
        // A part burning out fails the case, except the fuse a `blown` check waits for.
        let expected = |i: usize| matches!(case.check, Check::Blown { .. }) && targets.contains(&i);
        if let Some(burnt) = b.parts.iter().enumerate().find(|(i, _)| live.reading(&b, *i).burnt && !expected(*i)) {
            let what = match burnt.1.kind {
                Kind::Fuse { .. } => "blew",
                Kind::Capacitor { .. } => "vented: its voltage was too high or the wrong way round",
                _ => "burnt out",
            };
            return Outcome::fail(format!("{} {what}.", burnt.1.label));
        }
        if t + DT < start {
            continue;
        }
        let since = t + DT - start;
        match &case.check {
            Check::Lasts { .. } => {
                if targets.iter().any(|&i| live.reading(&b, i).amps < UNLIT) {
                    ended = Some(since);
                    break;
                }
            }
            Check::Blinks { .. } => {
                let i = targets[0];
                let on = if matches!(b.parts[i].kind, Kind::Relay { .. }) { live.energised(i) } else { live.reading(&b, i).amps >= UNLIT };
                if since >= BLINK_START && was_on == Some(false) && on {
                    rises.push(since);
                }
                was_on = Some(on);
            }
            Check::Reaches { volts, .. } => {
                let v = live.reading(&b, targets[0]).volts;
                let was = *from.get_or_insert(v);
                if (was - volts) * (v - volts) <= 0.0 {
                    ended = Some(since);
                    break;
                }
            }
            _ => {}
        }
    }
    let after = match case.events.last() {
        Some((_, label, v)) => {
            let button = b.part(label).is_some_and(|i| matches!(b.parts[i].kind, Kind::Button { .. }));
            let what = match (button, *v > 0.5) {
                (true, true) => "is pressed",
                (true, false) => "is let go",
                (false, true) => "goes on",
                (false, false) => "goes off",
            };
            format!(" after {label} {what}")
        }
        None => String::new(),
    };
    // "in 1.10 s" from the start, "1.10 s after SW1 goes off" from an event.
    let when = |s: f64| if after.is_empty() { format!("in {}", secs(s)) } else { format!("{}{after}", secs(s)) };
    match &case.check {
        Check::Lasts { min, max, .. } => {
            let label = names;
            return match ended {
                Some(s) if s < 0.05 && s < *min => Outcome {
                    pass: false,
                    measured: Some(s),
                    why: format!("{label} goes out at once{after}; it should stay lit at least {}.", secs(*min)),
                },
                Some(s) if s < *min => Outcome {
                    pass: false,
                    measured: Some(s),
                    why: format!("{label} goes out {}{after}; it should stay lit at least {}.", secs(s), secs(*min)),
                },
                Some(s) => Outcome { pass: true, measured: Some(s), why: format!("{label} stays lit {}{after}.", secs(s)) },
                None => Outcome {
                    pass: false,
                    measured: None,
                    why: format!("{label} is still lit {}{after}; it should go out within {}.", secs(*max + 0.5), secs(*max)),
                },
            };
        }
        Check::Blinks { min, max, .. } => {
            let label = names;
            let relay = matches!(b.parts[targets[0]].kind, Kind::Relay { .. });
            let (blinks, blinking) = if relay { ("chatters", "chattering") } else { ("blinks", "blinking") };
            // Slow blinks read as a period, quick ones as a rate.
            let range = if *max < 1.0 {
                format!("once every {} to {}", secs(1.0 / max), secs(1.0 / min))
            } else {
                format!("{} to {} times a second", hz(*min), hz(*max))
            };
            if rises.len() < 2 {
                let state = match (relay, was_on == Some(true), rises.len()) {
                    (_, _, 1) => format!("{blinks} only once in {}", secs(BLINK_WATCH)),
                    (true, true, _) => "stays pulled in".into(),
                    (true, false, _) => "stays at rest".into(),
                    (false, true, _) => "stays lit".into(),
                    (false, false, _) => "stays dark".into(),
                };
                return Outcome::fail(format!("{label} {state}{after}; it should be {blinking} {range}."));
            }
            let rate = (rises.len() - 1) as f64 / (rises[rises.len() - 1] - rises[0]);
            let ok = (*min..=*max).contains(&rate);
            let says = if rate < 1.0 { format!("once every {}", secs(1.0 / rate)) } else { format!("{} times a second", hz(rate)) };
            let why = if ok { format!("{label} {blinks} {says}.") } else { format!("{label} {blinks} {says}; this job wants {range}.") };
            return Outcome { pass: ok, measured: Some(rate), why };
        }
        Check::Reaches { volts, min, max, .. } => {
            let label = names;
            let v = si(*volts, "V");
            return match ended {
                Some(s) if s < *min || s > *max => Outcome {
                    pass: false,
                    measured: Some(s),
                    why: format!("{label} gets to {v} {}; this job wants {} to {}.", when(s), secs(*min), secs(*max)),
                },
                Some(s) => Outcome { pass: true, measured: Some(s), why: format!("{label} gets to {v} {}.", when(s)) },
                None => {
                    let now = si(live.reading(&b, targets[0]).volts, "V");
                    Outcome { pass: false, measured: None, why: format!("{label} never gets to {v}; it is at {now} {}.", when(*max + 0.5)) }
                }
            };
        }
        _ => {}
    }
    let outcomes: Vec<Outcome> = targets.iter().map(|&t| judge(&case.check, &b.parts[t].label, live.reading(&b, t))).collect();
    match outcomes.iter().find(|o| !o.pass) {
        Some(fail) => fail.clone(),
        None => Outcome {
            pass: true,
            measured: outcomes[0].measured,
            why: outcomes.iter().map(|o| o.why.trim_end_matches('.')).collect::<Vec<_>>().join("; ") + ".",
        },
    }
}

/// The board as a case sets it up: its battery voltage, its switches and buttons.
fn with_settings(board: &Board, case: &Case) -> Board {
    let mut b = board.clone();
    for part in &mut b.parts {
        match &mut part.kind {
            Kind::Battery { volts, .. } => {
                if let Some(v) = case.battery {
                    *volts = v;
                }
            }
            kind => {
                let Some(&(_, v)) = case.set.iter().find(|(l, _)| *l == part.label) else { continue };
                *kind = set_kind(*kind, v);
            }
        }
    }
    b
}

/// A part set as a case says: a switch or button on (1) or off (0), an LDR's lux, a
/// thermistor's °C, a pot's turn.
fn set_kind(kind: Kind, v: f64) -> Kind {
    match kind {
        Kind::Switch { .. } => Kind::Switch { closed: v > 0.5 },
        Kind::Button { nc, .. } => Kind::Button { held: v > 0.5, nc },
        Kind::Ldr { .. } => Kind::Ldr { lux: v },
        Kind::Ntc { .. } => Kind::Ntc { celsius: v },
        Kind::Pot { ohms, .. } => Kind::Pot { ohms, turn: v.clamp(0.0, 1.0) },
        k => k,
    }
}

/// A rate as the bench says it: "1.2", "75".
pub fn hz(f: f64) -> String {
    if f < 10.0 { format!("{f:.1}") } else { format!("{f:.0}") }
}

/// Seconds as the bench says them: "0.85 s", "12 s".
pub fn secs(s: f64) -> String {
    if s < 10.0 { format!("{s:.2} s") } else { format!("{s:.0} s") }
}

/// The current the sources give, amps, once a case has settled: what drains the battery.
pub fn drawn(board: &Board, case: &Case) -> f64 {
    let b = with_settings(board, case);
    let mut live = Live::build(&b);
    for _ in 0..(SETTLE / DT).round() as usize {
        if live.sim.step(DT).is_err() {
            return 0.0;
        }
    }
    (0..b.parts.len()).filter(|&i| b.parts[i].kind.is_source()).map(|i| live.reading(&b, i).amps.max(0.0)).sum()
}

/// One part against a check.
fn judge(check: &Check, label: &str, r: crate::circuit::Reading) -> Outcome {
    match check {
        Check::Lit { min, max, .. } => {
            let pass = r.amps >= *min && r.amps <= *max;
            // A pass reads as one, even glowing below what counts as lit elsewhere.
            let why = if pass && r.amps < 10e-6 {
                format!("{label} is still dark.")
            } else if pass && r.amps < UNLIT {
                format!("{label} glows dimly at {}.", si(r.amps, "A"))
            } else if pass {
                format!("{label} lit at {}.", si(r.amps, "A"))
            } else if r.amps < UNLIT {
                format!("{label} stays dark; it should carry {} to {}.", si(*min, "A"), si(*max, "A"))
            } else {
                format!("{label} carries {}; this job wants {} to {}.", si(r.amps, "A"), si(*min, "A"), si(*max, "A"))
            };
            Outcome { pass, measured: Some(r.amps), why }
        }
        Check::Dark { .. } => {
            let pass = r.amps < UNLIT;
            let why = if pass { format!("{label} is dark.") } else { format!("{label} lights at {}; it should be dark.", si(r.amps, "A")) };
            Outcome { pass, measured: Some(r.amps), why }
        }
        Check::Volts { min, max, .. } => {
            let pass = r.volts >= *min && r.volts <= *max;
            let why = if pass {
                format!("{label} reads {}.", si(r.volts, "V"))
            } else {
                format!("{label} reads {}; this job wants {} to {}.", si(r.volts, "V"), si(*min, "V"), si(*max, "V"))
            };
            Outcome { pass, measured: Some(r.volts), why }
        }
        Check::Lasts { .. } | Check::Reaches { .. } | Check::Blinks { .. } => unreachable!("timed checks are judged in run_case"),
        Check::Blown { .. } => {
            let why = if r.burnt { format!("{label} blew and cut the fault off.") } else { format!("{label} held through the fault; it should have blown.") };
            Outcome { pass: r.burnt, measured: Some(r.amps), why }
        }
    }
}

/// A value with an SI prefix and three significant figures: `si(0.0091, "A")` is "9.10 mA".
pub fn si(v: f64, unit: &str) -> String {
    let a = v.abs();
    let (scale, prefix) = if a >= 1e6 {
        (1e-6, "M")
    } else if a >= 1e3 {
        (1e-3, "k")
    } else if a == 0.0 || a >= 1.0 {
        (1.0, "")
    } else if a >= 1e-3 {
        (1e3, "m")
    } else if a >= 1e-6 {
        (1e6, "µ")
    } else {
        (1e9, "n")
    };
    let s = v * scale;
    let digits = if s.abs() >= 100.0 { 0 } else if s.abs() >= 10.0 { 1 } else { 2 };
    format!("{s:.digits$} {prefix}{unit}")
}

/// Something wrong with a build.
#[derive(Clone, Debug, PartialEq)]
pub struct Hint {
    /// The first thing to say: what's wrong, in general.
    pub nudge: String,
    /// The specific problem.
    pub detail: String,
    /// How to fix it.
    pub fix: String,
    /// Tiles to highlight.
    pub tiles: Vec<Pos>,
}

fn hint(nudge: String, detail: String, fix: String, tiles: Vec<Pos>) -> Hint {
    Hint { nudge, detail, fix, tiles }
}

/// What's wrong with a build, most fundamental first. Empty when nothing is.
pub fn diagnose(b: &Board) -> Vec<Hint> {
    let live = Live::build(b);
    let net = &live.net;
    let mut hints = Vec::new();
    let others = |part: usize, pin: usize| net.pins_on(net.pin_net[part][pin]).into_iter().filter(|&(p, k)| (p, k) != (part, pin)).count();

    // 1. Power.
    let sources: Vec<usize> = (0..b.parts.len()).filter(|&i| b.parts[i].kind.is_source()).collect();
    if sources.is_empty() {
        hints.push(hint(
            "Nothing on the board has power.".into(),
            "There's no battery or supply on the board.".into(),
            "Place a battery from the drawer, then join its terminals to the circuit.".into(),
            vec![],
        ));
        return hints;
    }
    for &s in &sources {
        let p = &b.parts[s];
        if net.pin_net[s][0] == net.pin_net[s][1] {
            hints.push(hint(
                format!("{}'s + and - are joined together: a dead short.", p.label),
                format!("Follow the copper from {}'s + terminal: it comes back to its - terminal without passing through a part.", p.label),
                "Cut the trace that joins them, so every path from + to - goes through something.".into(),
                vec![p.pos],
            ));
            continue;
        }
        for pin in 0..2 {
            if others(s, pin) == 0 {
                hints.push(hint(
                    format!("{} isn't connected yet.", p.label),
                    format!("Nothing is joined to {}'s {}.", p.label, p.kind.pin_name(pin)),
                    format!("Lay a trace from {}'s {} to the circuit.", p.label, p.kind.pin_name(pin)),
                    vec![p.pos],
                ));
            }
        }
    }
    if !hints.is_empty() {
        return hints;
    }

    // 2. Parts that do nothing: both pins on one net.
    for (i, p) in b.parts.iter().enumerate() {
        if p.kind.pins() == 2 && !p.kind.is_source() && net.pin_net[i][0] == net.pin_net[i][1] {
            hints.push(hint(
                format!("{} isn't doing anything.", p.label),
                format!("Both of {}'s pins are on the same copper, so current goes around it, not through it.", p.label),
                format!("Cut the trace that runs from one side of {} to the other.", p.label),
                vec![p.pos],
            ));
        }
    }

    // A relay's coil or contact with both its legs on one copper.
    for (i, p) in b.parts.iter().enumerate() {
        if !matches!(p.kind, Kind::Relay { .. }) {
            continue;
        }
        for (legs, what) in [((0, 1), "coil"), ((2, 3), "contact")] {
            if net.pin_net[i][legs.0] == net.pin_net[i][legs.1] {
                hints.push(hint(
                    format!("{}'s {what} isn't doing anything.", p.label),
                    format!("Both of {}'s {what} legs are on the same copper, so current goes around the {what}, not through it.", p.label),
                    format!("Cut the trace that runs from one {what} leg of {} to the other.", p.label),
                    vec![p.pos],
                ));
            }
        }
    }

    // 3. Pins with nothing else on their net.
    for (i, p) in b.parts.iter().enumerate() {
        if p.kind.is_source() {
            continue;
        }
        for pin in 0..p.kind.pins() {
            if net.pin_net[i][0..p.kind.pins()].iter().filter(|&&n| n == net.pin_net[i][pin]).count() > 1 {
                continue;
            }
            // A pot used as a variable resistor leaves one end free, rightly.
            if matches!(p.kind, Kind::Pot { .. }) && pin < 2 {
                continue;
            }
            if others(i, pin) == 0 {
                hints.push(hint(
                    format!("{} is only half connected.", p.label),
                    format!("{}'s {} doesn't reach any other part.", p.label, p.kind.pin_name(pin)),
                    format!("Lay a trace from {}'s {} to where it should go.", p.label, p.kind.pin_name(pin)),
                    vec![p.pos],
                ));
            }
        }
    }

    // 4. Copper that ends in the air.
    if !net.dangling.is_empty() {
        let tiles: Vec<Pos> = net.dangling.iter().map(|(p, _)| *p).collect();
        let n = tiles.len();
        hints.push(hint(
            if n == 1 { "A trace ends in the air.".into() } else { format!("{n} traces end in the air.") },
            "Copper that stops at an empty tile doesn't connect to anything there.".into(),
            "Finish the trace to a part or another trace, or erase the loose end.".into(),
            tiles,
        ));
    }
    if !hints.is_empty() {
        return hints;
    }

    // 5. One powered trial: what the build does when switched on as it stands.
    let mut live = live;
    let mut peak = vec![0.0f64; b.parts.len()];
    for _ in 0..(SETTLE / DT) as usize {
        if live.sim.step(DT).is_err() {
            hints.push(hint(
                "Two sources are fighting.".into(),
                "Two batteries or supplies are joined directly, each trying to hold its own voltage.".into(),
                "Put a part between them, or remove one.".into(),
                sources.iter().map(|&s| b.parts[s].pos).collect(),
            ));
            return hints;
        }
        for (i, pk) in peak.iter_mut().enumerate() {
            let r = live.reading(b, i);
            if !r.burnt {
                *pk = pk.max(r.watts.abs());
            }
        }
    }
    for (i, p) in b.parts.iter().enumerate() {
        let r = live.reading(b, i);
        if r.burnt {
            let (detail, fix) = match p.kind {
                Kind::Led(_) => (
                    format!("{} took too much current and burnt out. An LED needs something to limit its current.", p.label),
                    format!("Put a resistor in series with {}.", p.label),
                ),
                Kind::Resistor { watts, .. } | Kind::Lamp { watts, .. } => (
                    format!("{} had to turn {} into heat; it's rated for {}.", p.label, si(peak[i], "W"), si(watts, "W")),
                    format!("Use a larger value for {}, or a part rated for more power.", p.label),
                ),
                Kind::Npn { .. } => (
                    format!("{} carried more than it can: 100 mA through its collector, or 0.625 W of heat.", p.label),
                    format!("Feed {}'s base through a resistor, and put a resistor or the load in its collector's path.", p.label),
                ),
                Kind::Diode => (
                    format!("More than {} flowed through {} and cooked it: a short, with only the diode in the way.", si(1.0, "A"), p.label),
                    format!("Find what draws the current through {}: a diode needs something else to limit it.", p.label),
                ),
                Kind::Relay { volts, .. } => (
                    format!("{}'s coil is made for {} and takes no more than {} for long; it had {} across it.", p.label, si(volts, "V"), si(volts * crate::solver::RELAY_MAX, "V"), si(r.volts.abs().max(peak[i].sqrt() * volts / crate::solver::RELAY_WATTS.sqrt()), "V")),
                    format!("Use a relay whose coil is rated for the voltage {} gets.", p.label),
                ),
                Kind::Capacitor { .. } => (
                    format!("{} vented: it was held past its rated voltage, or charged the wrong way round.", p.label),
                    format!("Turn {} so its striped leg goes toward -, or use one rated for more volts.", p.label),
                ),
                Kind::Fuse { amps } => (
                    format!("More than {} flowed through {}, so it melted: a short, or a load bigger than the fuse.", si(amps, "A"), p.label),
                    "Find what draws the current. If the load is right, use the next fuse up that still blows on a fault.".into(),
                ),
                _ => (format!("{} burnt out.", p.label), format!("Check what {} is connected across.", p.label)),
            };
            let what = if matches!(p.kind, Kind::Fuse { .. }) { "blew" } else { "burnt out" };
            hints.push(hint(format!("{} {what}.", p.label), detail, fix, vec![p.pos]));
        } else if let Kind::Diode = p.kind
            && r.volts < -1.0
            && sources.iter().map(|&s| live.reading(b, s).amps.abs()).sum::<f64>() < 1e-6
        {
            hints.push(hint(
                format!("{} is blocking.", p.label),
                format!("{} only passes current from its anode to its cathode (the band); its anode is {} below its cathode, so it blocks, and nothing else flows.", p.label, si(-r.volts, "V")),
                format!("Turn {} round so its band faces toward -.", p.label),
                vec![p.pos],
            ));
        } else if let Kind::Led(_) = p.kind
            && r.volts < -1.0
        {
            hints.push(hint(
                format!("{} is in the circuit but dark.", p.label),
                format!(
                    "{} is backwards: current only flows from its long leg (anode, +) to its short leg (cathode, -). Its + side is {} below its - side.",
                    p.label,
                    si(-r.volts, "V")
                ),
                format!("Turn {} round: long leg toward +, short leg toward -.", p.label),
                vec![p.pos],
            ));
        }
    }
    // A relay whose coil gets some voltage, but not enough to pull in.
    for (i, p) in b.parts.iter().enumerate() {
        if let Kind::Relay { volts, .. } = p.kind {
            let v = live.reading(b, i).volts.abs();
            if !live.energised(i) && v > crate::solver::RELAY_DROP * volts && v < crate::solver::RELAY_PULL * volts {
                hints.push(hint(
                    format!("{} never pulls in.", p.label),
                    format!("{}'s coil gets {}; a {} relay needs at least {} to be sure of pulling in.", p.label, si(v, "V"), si(volts, "V"), si(crate::solver::RELAY_PULL * volts, "V")),
                    format!("Give {}'s coil more voltage, or use a relay rated for what it gets.", p.label),
                    vec![p.pos],
                ));
            }
        }
    }
    if hints.is_empty() {
        let delivered: f64 = sources.iter().map(|&s| live.reading(b, s).amps.abs()).sum();
        if delivered < 1e-6 {
            let open: Vec<&str> = b
                .parts
                .iter()
                .filter(|p| matches!(p.kind, Kind::Switch { closed: false } | Kind::Button { held: false, nc: false }))
                .map(|p| p.label.as_str())
                .collect();
            let detail = if open.is_empty() {
                "There's no closed loop from + back to -.".to_string()
            } else {
                format!("There's no closed loop from + back to -. Open right now: {}.", open.join(", "))
            };
            hints.push(hint(
                "No current flows.".into(),
                detail,
                "Follow the copper from + through every part and back to -: somewhere the loop is broken.".into(),
                vec![],
            ));
        }
    }
    hints
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::tests::torch;
    use crate::circuit::{Colour, Dir};

    fn lit(min: f64, max: f64) -> Check {
        Check::Lit { part: "LED1".into(), min, max }
    }

    fn case(battery: Option<f64>, set: &[(&str, bool)], check: Check) -> Case {
        Case::steady("test", battery, set.iter().map(|(l, v)| (l.to_string(), if *v { 1.0 } else { 0.0 })).collect(), check)
    }

    #[test]
    fn torch_passes_its_spec_for_fresh_and_tired_batteries() {
        let b = torch();
        for v in [4.5, 3.6] {
            let o = run_case(&b, &case(Some(v), &[], lit(0.005, 0.020)));
            assert!(o.pass, "{v} V: {}", o.why);
        }
        let off = run_case(&b, &case(None, &[("SW1", false)], Check::Dark { part: "LED1".into() }));
        assert!(off.pass, "{}", off.why);
    }

    #[test]
    fn a_failed_case_says_what_it_measured() {
        let o = run_case(&torch(), &case(None, &[], lit(0.015, 0.020)));
        assert!(!o.pass);
        assert_eq!(o.why, "LED1 carries 11.3 mA; this job wants 15.0 mA to 20.0 mA.");
    }

    #[test]
    fn a_working_torch_has_no_hints() {
        assert_eq!(diagnose(&torch()), vec![]);
    }

    #[test]
    fn no_source_is_the_first_thing_said() {
        let mut b = torch();
        b.erase(b.parts[b.part("BT1").unwrap()].pos);
        let h = diagnose(&b);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].nudge, "Nothing on the board has power.");
    }

    #[test]
    fn a_dead_short_is_named() {
        let b = Board::parse(&["┌B┐", "└─┘"], &[('B', Kind::Battery { volts: 9.0, r_int: 1.0 }, Dir::W)]).unwrap();
        let h = diagnose(&b);
        assert_eq!(h[0].nudge, "BT1's + and - are joined together: a dead short.");
    }

    #[test]
    fn a_bypassed_resistor_is_named() {
        // A trace under the resistor's row joins both its sides.
        let b = Board::parse(
            &["┌┬R┬┐", "B└─┘L", "└───┘"],
            &[
                ('B', Kind::Battery { volts: 4.5, r_int: 0.45 }, Dir::N),
                ('R', Kind::Resistor { ohms: 220.0, watts: 0.25 }, Dir::W),
                ('L', Kind::Led(Colour::Red), Dir::N),
            ],
        )
        .unwrap();
        let h = diagnose(&b);
        assert_eq!(h[0].nudge, "R1 isn't doing anything.");
    }

    #[test]
    fn a_half_connected_part_is_named() {
        let b = Board::parse(
            &["┌R─┐", "B  L", "└──┘", "  C "],
            &[
                ('B', Kind::Battery { volts: 4.5, r_int: 0.45 }, Dir::N),
                ('R', Kind::Resistor { ohms: 220.0, watts: 0.25 }, Dir::W),
                ('L', Kind::Led(Colour::Red), Dir::N),
                ('C', Kind::Capacitor { farads: 1e-6, volts: 25.0 }, Dir::N),
            ],
        )
        .unwrap();
        let h = diagnose(&b);
        // The trace above the capacitor runs east-west, so neither of its pins meets anything.
        assert!(h.iter().any(|h| h.nudge == "C1 is only half connected."), "{h:?}");
    }

    #[test]
    fn a_loose_trace_end_is_named() {
        let b = Board::parse(
            &["┌R─┐", "B  L", "└──┤ "],
            &[
                ('B', Kind::Battery { volts: 4.5, r_int: 0.45 }, Dir::N),
                ('R', Kind::Resistor { ohms: 220.0, watts: 0.25 }, Dir::W),
                ('L', Kind::Led(Colour::Red), Dir::N),
            ],
        )
        .unwrap();
        let h = diagnose(&b);
        assert_eq!(h[0].nudge, "A trace ends in the air.");
        assert_eq!(h[0].tiles, vec![Pos { x: 3, y: 2 }]);
    }

    #[test]
    fn a_backwards_led_is_named() {
        let b = Board::parse(
            &["┌R─┐", "B  L", "└──┘"],
            &[
                ('B', Kind::Battery { volts: 4.5, r_int: 0.45 }, Dir::N),
                ('R', Kind::Resistor { ohms: 220.0, watts: 0.25 }, Dir::W),
                ('L', Kind::Led(Colour::Red), Dir::S),
            ],
        )
        .unwrap();
        let h = diagnose(&b);
        assert_eq!(h[0].nudge, "LED1 is in the circuit but dark.");
        assert!(h[0].detail.contains("backwards"));
    }

    #[test]
    fn an_led_without_a_resistor_burns_and_says_why() {
        let b = Board::parse(
            &["┌──┐", "B  L", "└──┘"],
            &[('B', Kind::Battery { volts: 4.5, r_int: 0.45 }, Dir::N), ('L', Kind::Led(Colour::Red), Dir::N)],
        )
        .unwrap();
        let h = diagnose(&b);
        assert_eq!(h[0].nudge, "LED1 burnt out.");
        assert_eq!(h[0].fix, "Put a resistor in series with LED1.");
    }

    #[test]
    fn an_overloaded_resistor_names_its_power() {
        let b = Board::parse(
            &["┌──┐", "B  R", "└──┘"],
            &[
                ('B', Kind::Battery { volts: 9.0, r_int: 0.0 }, Dir::N),
                ('R', Kind::Resistor { ohms: 100.0, watts: 0.25 }, Dir::N),
            ],
        )
        .unwrap();
        let h = diagnose(&b);
        assert_eq!(h[0].detail, "R1 had to turn 810 mW into heat; it's rated for 250 mW.");
    }

    #[test]
    fn an_open_switch_is_named_when_nothing_flows() {
        let mut b = torch();
        let sw = b.part("SW1").unwrap();
        b.parts[sw].kind = Kind::Switch { closed: false };
        let h = diagnose(&b);
        assert_eq!(h[0].nudge, "No current flows.");
        assert!(h[0].detail.ends_with("Open right now: SW1."), "{}", h[0].detail);
    }

    #[test]
    fn si_prefixes() {
        assert_eq!(si(0.0091, "A"), "9.10 mA");
        assert_eq!(si(220.0, "Ω"), "220 Ω");
        assert_eq!(si(4.7e-6, "F"), "4.70 µF");
        assert_eq!(si(0.0, "V"), "0.00 V");
        assert_eq!(si(4700.0, "Ω"), "4.70 kΩ");
    }

    #[test]
    fn a_relay_short_of_voltage_or_cooked_says_so() {
        let buzzer = |volts: f64| {
            let rows = ["┌─P┐┌┐", "│..└K┘", "B...│.", "└───┘."];
            Board::parse(
                &rows,
                &[
                    ('B', Kind::Battery { volts: 4.5, r_int: 0.45 }, Dir::N),
                    ('P', Kind::Button { held: true, nc: false }, Dir::W),
                    ('K', Kind::Relay { volts, nc: true }, Dir::N),
                ],
            )
            .unwrap()
        };
        let h = diagnose(&buzzer(12.0));
        assert!(h.iter().any(|h| h.nudge == "K1 never pulls in."), "{h:?}");
        assert!(diagnose(&buzzer(5.0)).iter().all(|h| !h.nudge.contains("K1")));
        let mut b = buzzer(5.0);
        let (bt, k) = (b.part("BT1").unwrap(), b.part("K1").unwrap());
        b.parts[bt].kind = Kind::Battery { volts: 12.0, r_int: 0.05 };
        // Cooked before the trial: it burns in about 0.75 s, longer than the trial runs.
        b.parts[k].burnt = true;
        let h = diagnose(&b);
        assert!(h.iter().any(|h| h.nudge == "K1 burnt out." && h.detail.contains("made for 5.00 V")), "{h:?}");
    }

    #[test]
    fn a_diode_backwards_says_it_is_blocking() {
        let rows = ["┌D─┐", "B..L", "└S─┘"];
        let with = |d: Dir| {
            let b = Board::parse(
                &rows,
                &[
                    ('B', Kind::Battery { volts: 4.5, r_int: 0.45 }, Dir::N),
                    ('D', Kind::Diode, d),
                    ('L', Kind::Lamp { ohms: 15.0, watts: 1.35 }, Dir::N),
                    ('S', Kind::Switch { closed: true }, Dir::W),
                ],
            )
            .unwrap();
            diagnose(&b)
        };
        assert!(with(Dir::W).is_empty(), "{:?}", with(Dir::W));
        assert!(with(Dir::E).iter().any(|h| h.nudge == "D1 is blocking."), "{:?}", with(Dir::E));
    }
}
