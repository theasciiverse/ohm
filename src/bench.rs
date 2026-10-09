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
}

/// One condition the build must meet: how the bench sets the board up, and what it checks.
#[derive(Clone, Debug, PartialEq)]
pub struct Case {
    pub name: String,
    /// Every battery set to this voltage (a fresh cell, a tired one).
    pub battery: Option<f64>,
    /// Switches and buttons by label: closed or held.
    pub set: Vec<(String, bool)>,
    pub check: Check,
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
    if let Some((label, _)) = case.set.iter().find(|(l, _)| board.part(l).is_none()) {
        let what = if label.starts_with("PB") { "a push button" } else { "a switch" };
        return Outcome::fail(format!("{label} isn't on the board: this job needs {what}."));
    }
    let b = with_settings(board, case);
    // A check can name several parts joined with `+` (`LED1+LED2`): each must pass.
    let names = match &case.check {
        Check::Lit { part, .. } | Check::Dark { part } => part,
        Check::Volts { point, .. } => point,
    };
    let mut targets = Vec::new();
    for name in names.split('+') {
        match b.part(name) {
            Some(i) => targets.push(i),
            None => return Outcome::fail(format!("{name} isn't on the board.")),
        }
    }
    let mut live = Live::build(&b);
    for _ in 0..(SETTLE / DT).round() as usize {
        if live.sim.step(DT).is_err() {
            return Outcome::fail("The circuit can't be solved: two sources are fighting.".into());
        }
        if let Some(burnt) = b.parts.iter().enumerate().find(|(i, _)| live.reading(&b, *i).burnt) {
            return Outcome::fail(format!("{} burnt out.", burnt.1.label));
        }
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
            Kind::Switch { closed: on } | Kind::Button { held: on } => {
                if let Some((_, v)) = case.set.iter().find(|(l, _)| *l == part.label) {
                    *on = *v;
                }
            }
            _ => {}
        }
    }
    b
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
            let why = if r.amps < UNLIT {
                format!("{label} stays dark; it should carry {} to {}.", si(*min, "A"), si(*max, "A"))
            } else if pass {
                format!("{label} lit at {}.", si(r.amps, "A"))
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

    // 3. Pins with nothing else on their net.
    for (i, p) in b.parts.iter().enumerate() {
        if p.kind.is_source() {
            continue;
        }
        for pin in 0..p.kind.pins() {
            if net.pin_net[i][0..p.kind.pins()].iter().filter(|&&n| n == net.pin_net[i][pin]).count() > 1 {
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
                _ => (format!("{} burnt out.", p.label), format!("Check what {} is connected across.", p.label)),
            };
            hints.push(hint(format!("{} burnt out.", p.label), detail, fix, vec![p.pos]));
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
    if hints.is_empty() {
        let delivered: f64 = sources.iter().map(|&s| live.reading(b, s).amps.abs()).sum();
        if delivered < 1e-6 {
            let open: Vec<&str> = b
                .parts
                .iter()
                .filter(|p| matches!(p.kind, Kind::Switch { closed: false } | Kind::Button { held: false }))
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
        Case { name: "test".into(), battery, set: set.iter().map(|(l, v)| (l.to_string(), *v)).collect(), check }
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
                ('C', Kind::Capacitor { farads: 1e-6 }, Dir::N),
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
}
