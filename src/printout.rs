//! The printed bench (`ohm --bench`): solver measurements, boards run through spec cases and
//! diagnosis hints, as text, so the numbers and the words can be checked by eye.

use crate::bench::{self, Case, Check, diagnose, run_case};
use crate::circuit::{Board, Cell, Colour, Dir, Kind, Live, Pos};
use crate::solver::{Circuit, GROUND, Led};

pub fn run() {
    println!("Ohm solver bench\n");

    // A torch: three fresh AA cells, a resistor, a red LED and a switch.
    let mut c = Circuit::new();
    let pos = c.node();
    let r_out = c.node();
    let a = c.node();
    let bat = c.battery(pos, GROUND, 4.5, 0.45);
    let sw = c.switch(pos, r_out, true);
    let r = c.resistor_rated(r_out, a, 220.0, 0.25);
    let led = c.led(a, GROUND, Led::RED);
    c.solve_dc().expect("torch solves");
    println!("Torch, 4.5 V battery, 220 ohm, red LED");
    println!("  battery  {:6.3} V  {:6.2} mA", c.voltage(pos), c.current(bat) * 1e3);
    println!("  resistor {:6.2} mW of its 250 mW", c.power(r) * 1e3);
    println!("  LED      {:6.3} V  {:6.2} mA", c.voltage(a), c.current(led) * 1e3);
    c.set_switch(sw, false);
    c.solve_dc().expect("torch solves");
    println!("  switch off: LED {:.1e} A (leakage through the solver's tiny ground conductance)\n", c.current(led));

    // Every LED colour on a current-limited bench supply.
    println!("LED colours on a 5 V bench supply, 20 mA limit");
    for (name, colour) in [("red", Led::RED), ("yellow", Led::YELLOW), ("green", Led::GREEN), ("blue", Led::BLUE)] {
        let mut c = Circuit::new();
        let pos = c.node();
        let s = c.supply(pos, GROUND, 5.0, 0.020);
        c.led(pos, GROUND, colour);
        c.solve_dc().expect("led solves");
        println!("  {name:6} {:5.3} V  limiting: {}", c.voltage(pos), c.limiting(s));
    }
    println!();

    // An RC delay: 10k and 100 uF, tau = 1 s.
    println!("RC charge, 5 V through 10k into 100 uF (tau 1 s)");
    let mut c = Circuit::new();
    let pos = c.node();
    let a = c.node();
    c.battery(pos, GROUND, 5.0, 0.0);
    c.resistor(pos, a, 10_000.0);
    c.capacitor(a, GROUND, 100e-6);
    for second in 1..=5 {
        for _ in 0..1000 {
            c.step(0.001).expect("rc steps");
        }
        println!("  t = {second} s  {:5.3} V", c.voltage(a));
    }
    println!();

    // An LED with no resistor, watched as it dies.
    println!("Red LED straight across 5 V");
    let mut c = Circuit::new();
    let pos = c.node();
    c.battery(pos, GROUND, 5.0, 0.0);
    let led = c.led(pos, GROUND, Led::RED);
    let mut ms = 0;
    while !c.burnt(led) && ms < 1000 {
        c.step(0.001).expect("led steps");
        ms += 1;
        println!("  {ms:3} ms  {:6.1} mA  heat {:4.2}", c.current(led) * 1e3, c.heat(led));
    }
    println!("  burnt out after {ms} ms");
    c.repair_all();
    println!("  repaired: burnt = {}", c.burnt(led));

    println!();
    boards();
}

/// Prints a board as drawn, one character per tile.
fn show(rows: &[&str]) {
    for r in rows {
        println!("    {r}");
    }
}

fn lit(part: &str, min: f64, max: f64) -> Check {
    Check::Lit { part: part.into(), min, max }
}

fn case(name: &str, battery: Option<f64>, set: &[(&str, bool)], check: Check) -> Case {
    Case::steady(name, battery, set.iter().map(|(l, v)| (l.to_string(), if *v { 1.0 } else { 0.0 })).collect(), check)
}

fn run_cases(board: &Board, cases: &[Case]) {
    for c in cases {
        let o = run_case(board, c);
        println!("  {:14} {}  {}", c.name, if o.pass { "pass" } else { "FAIL" }, o.why);
    }
}

/// Boards drawn as text, run through spec cases and diagnosed.
fn boards() {
    let torch_rows = ["┌R─┐", "B  L", "└S─┘"];
    let legend = [
        ('B', Kind::Battery { volts: 4.5, r_int: 0.45 }, Dir::N),
        ('R', Kind::Resistor { ohms: 220.0, watts: 0.25 }, Dir::W),
        ('L', Kind::Led(Colour::Red), Dir::N),
        ('S', Kind::Switch { closed: true }, Dir::W),
        ('l', Kind::Led(Colour::Red), Dir::S),
        ('s', Kind::Switch { closed: false }, Dir::W),
    ];
    let torch = Board::parse(&torch_rows, &legend).expect("torch parses");
    println!("The torch as a board");
    show(&torch_rows);
    run_cases(
        &torch,
        &[
            case("fresh cells", Some(4.8), &[], lit("LED1", 0.005, 0.020)),
            case("tired cells", Some(3.3), &[], lit("LED1", 0.005, 0.020)),
            case("switched off", None, &[("SW1", false)], Check::Dark { part: "LED1".into() }),
        ],
    );
    println!();

    // A divider from E12 values: 9 V into 2.7k over 1.5k, measured at a test point.
    let divider_rows = ["┌──┐", "B  R", "│  ├T", "│  Q", "└──┘"];
    let divider = Board::parse(
        &divider_rows,
        &[
            ('B', Kind::Battery { volts: 9.0, r_int: 1.0 }, Dir::N),
            ('R', Kind::Resistor { ohms: 2700.0, watts: 0.25 }, Dir::N),
            ('Q', Kind::Resistor { ohms: 1500.0, watts: 0.25 }, Dir::N),
            ('T', Kind::TestPoint, Dir::W),
        ],
    )
    .expect("divider parses");
    println!("A divider, 9 V into 2.7k over 1.5k");
    show(&divider_rows);
    run_cases(&divider, &[case("3.3 V rail", None, &[], Check::Volts { point: "TP1".into(), min: 3.1, max: 3.5 })]);
    let mut live = Live::build(&divider);
    live.sim.solve_dc().expect("divider solves");
    println!("  every copper tile reads:");
    for y in 0..divider.h {
        let line: String = (0..divider.w)
            .map(|x| {
                let p = Pos { x, y };
                match divider.cell(p) {
                    Cell::Trace(_) => {
                        let net = Dir::ALL.iter().find_map(|&d| live.net.net_at(&divider, p, d)).unwrap();
                        format!("{:5.2}", live.net_volts(net))
                    }
                    Cell::Part(i) => format!("{:>5}", divider.parts[i].label),
                    _ => "     ".into(),
                }
            })
            .collect();
        println!("    {line}");
    }
    println!();

    // An RC delay on the board: 10k into 100 uF.
    let rc_rows = ["┌R─┐", "B  C", "└──┘"];
    let rc = Board::parse(
        &rc_rows,
        &[
            ('B', Kind::Battery { volts: 5.0, r_int: 0.0 }, Dir::N),
            ('R', Kind::Resistor { ohms: 10_000.0, watts: 0.25 }, Dir::W),
            ('C', Kind::Capacitor { farads: 100e-6, volts: 25.0 }, Dir::N),
        ],
    )
    .expect("rc parses");
    println!("A capacitor charging through 10k (tau 1 s)");
    show(&rc_rows);
    let mut live = Live::build(&rc);
    let c1 = rc.part("C1").unwrap();
    for second in 1..=3 {
        for _ in 0..1000 {
            live.sim.step(0.001).expect("rc steps");
        }
        println!("  t = {second} s  C1 at {}", bench::si(live.reading(&rc, c1).volts, "V"));
    }
    println!();

    // A porch lamp on a push button.
    let porch_rows = ["┌P─┐", "B  X", "└──┘"];
    let porch = Board::parse(
        &porch_rows,
        &[
            ('B', Kind::Battery { volts: 4.5, r_int: 0.45 }, Dir::N),
            ('P', Kind::Button { held: false, nc: false }, Dir::W),
            ('X', Kind::Lamp { ohms: 15.0, watts: 1.35 }, Dir::N),
        ],
    )
    .expect("porch parses");
    println!("A lamp on a push button");
    show(&porch_rows);
    run_cases(
        &porch,
        &[
            case("button held", None, &[("PB1", true)], lit("LP1", 0.2, 0.35)),
            case("let go", None, &[], Check::Dark { part: "LP1".into() }),
        ],
    );
    println!();

    // Every LED colour on a current-limited bench supply.
    println!("LED colours on a 5 V bench supply, 20 mA limit");
    for colour in [Colour::Red, Colour::Yellow, Colour::Green, Colour::Blue] {
        let b = Board::parse(
            &["┌P┐", "└L┘"],
            &[('P', Kind::Supply { volts: 5.0, limit: 0.020 }, Dir::E), ('L', Kind::Led(colour), Dir::E)],
        )
        .expect("bench parses");
        let mut live = Live::build(&b);
        live.sim.solve_dc().expect("led solves");
        let r = live.reading(&b, b.part("LED1").unwrap());
        println!("  {:6} {}  {}", colour.name(), bench::si(r.volts, "V"), bench::si(r.amps, "A"));
    }
    println!();

    let broken = [
        ("No resistor", vec!["┌──┐", "B  L", "└S─┘"]),
        ("LED in backwards", vec!["┌R─┐", "B  l", "└S─┘"]),
        ("A loose trace", vec!["┌R─┐", "B  L", "└S─┤"]),
        ("Switched off", vec!["┌R─┐", "B  L", "└s─┘"]),
    ];
    for (name, rows) in broken {
        let b = Board::parse(&rows, &legend).expect("board parses");
        println!("{name}");
        show(&rows);
        for h in diagnose(&b) {
            println!("  - {}\n    {}\n    {}", h.nudge, h.detail, h.fix);
        }
    }
}

