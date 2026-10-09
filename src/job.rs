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
//! `set LABEL on|off`), then one check (`lit LABEL MIN MAX`, `dark LABEL`, `volts LABEL MIN MAX`).

use crate::bench::{Case, Check};
use crate::circuit::{Board, Colour, Dir, Kind};

/// The jobs, in order, compiled in.
pub const JOBS: &[(&str, &str)] = &[
    ("bike-lamp", include_str!("../jobs/01-bike-lamp.txt")),
    ("porch-lamp", include_str!("../jobs/02-porch-lamp.txt")),
    ("two-doors", include_str!("../jobs/03-two-doors.txt")),
    ("dolls-house", include_str!("../jobs/04-dolls-house.txt")),
    ("christmas-star", include_str!("../jobs/05-christmas-star.txt")),
    ("cafe-sign", include_str!("../jobs/06-cafe-sign.txt")),
];

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
        Kind::Button { .. } => Kind::Button { held: false },
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
        Some("button") => (Kind::Button { held: false }, 1),
        Some("cap") => (Kind::Capacitor { farads: arg(1)? }, 2),
        Some("testpoint") => (Kind::TestPoint, 1),
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
        Kind::Button { .. } => "button".into(),
        Kind::Capacitor { farads } => format!("cap {farads}"),
        Kind::TestPoint => "testpoint".into(),
        Kind::Led(c) => format!("led {}", c.name()),
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
    let mut c = Case { name, battery: None, set: Vec::new(), check: Check::Dark { part: String::new() } };
    let mut checked = false;
    for p in parts {
        let w: Vec<&str> = p.split_whitespace().collect();
        match w.as_slice() {
            ["battery", v] => c.battery = Some(number(v)?),
            ["set", label, state] => c.set.push((label.to_string(), *state == "on")),
            ["lit", label, min, max] => {
                c.check = Check::Lit { part: label.to_string(), min: number(min)?, max: number(max)? };
                checked = true;
            }
            ["dark", label] => {
                c.check = Check::Dark { part: label.to_string() };
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
    const BUTTON: Kind = Kind::Button { held: false };
    const R: fn(f64) -> Kind = |ohms| Kind::Resistor { ohms, watts: 0.25 };

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
}
