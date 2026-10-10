//! The Master's Notebook: teaching pages on basic electronics, read from `book/`.
//!
//! ```text
//! title: Ohm's law
//! unlock: start                (or a job id: the page opens once that job is in the inbox)
//! para: A resistor resists current. ...     (one paragraph per line)
//! colours:                     (the colour code chart, every band colour with its digit)
//! resistor: 220                (a resistor drawn large with its bands, named)
//! source: Georg Simon Ohm, ... (where every fact on the page comes from)
//! part: B battery 4.5 0.45 N   (an example circuit, in the job file format)
//! grid:
//! ┌─R─┐
//! B...│
//! └───┘
//! ```

use crate::circuit::Board;
use crate::job::{dir, kind};

pub const PAGES: &[(&str, &str)] = &[
    ("current", include_str!("../book/01-current.txt")),
    ("voltage", include_str!("../book/02-voltage.txt")),
    ("ohms-law", include_str!("../book/03-ohms-law.txt")),
    ("switches", include_str!("../book/04-switches.txt")),
    ("series-parallel", include_str!("../book/05-series-parallel.txt")),
    ("leds", include_str!("../book/06-leds.txt")),
    ("power", include_str!("../book/07-power.txt")),
    ("colour-code", include_str!("../book/08-colour-code.txt")),
    ("batteries", include_str!("../book/09-batteries.txt")),
    ("measuring", include_str!("../book/10-measuring.txt")),
    ("dividers", include_str!("../book/11-dividers.txt")),
    ("potentiometers", include_str!("../book/12-potentiometers.txt")),
    ("sensors", include_str!("../book/13-sensors.txt")),
    ("fuses", include_str!("../book/14-fuses.txt")),
    ("capacitors", include_str!("../book/15-capacitors.txt")),
    ("time-constant", include_str!("../book/16-time-constant.txt")),
    ("discharge", include_str!("../book/17-discharge.txt")),
    ("relays", include_str!("../book/18-relays.txt")),
    ("self-holding", include_str!("../book/19-self-holding.txt")),
    ("relay-logic", include_str!("../book/20-relay-logic.txt")),
    ("buzzers", include_str!("../book/21-buzzers.txt")),
    ("diodes", include_str!("../book/22-diodes.txt")),
    ("transistors", include_str!("../book/23-transistors.txt")),
    ("transistor-switches", include_str!("../book/24-transistor-switches.txt")),
    ("transistor-sensors", include_str!("../book/25-transistor-sensors.txt")),
    ("flip-flops", include_str!("../book/26-flip-flops.txt")),
    ("blinkers", include_str!("../book/27-blinkers.txt")),
    ("one-shots", include_str!("../book/28-one-shots.txt")),
];

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Para(String),
    /// Each band colour beside its digit.
    Colours,
    /// A resistor of this many ohms, drawn large enough to read its bands.
    Resistor(f64),
}

#[derive(Clone, Debug)]
pub struct Page {
    pub id: String,
    pub title: String,
    /// The job that opens this page; `None` for pages open from the start.
    pub unlock: Option<String>,
    /// The page's text, charts and drawings, top to bottom.
    pub blocks: Vec<Block>,
    pub sources: Vec<String>,
    /// A small example circuit, live on the page.
    pub board: Option<Board>,
}

impl Page {
    pub fn parse(id: &str, text: &str) -> Result<Page, String> {
        let mut page = Page { id: id.into(), title: String::new(), unlock: None, blocks: Vec::new(), sources: Vec::new(), board: None };
        let mut legend = Vec::new();
        let mut grid: Option<Vec<String>> = None;
        for (n, line) in text.lines().enumerate() {
            let err = |e: String| format!("{id} line {}: {e}", n + 1);
            if let Some(rows) = &mut grid {
                if !line.trim().is_empty() {
                    rows.push(line.replace('.', " "));
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
                "title" => page.title = value.into(),
                "unlock" => page.unlock = (value != "start").then(|| value.to_string()),
                "para" => page.blocks.push(Block::Para(value.into())),
                "colours" => page.blocks.push(Block::Colours),
                "resistor" => page.blocks.push(Block::Resistor(crate::job::number(value).map_err(err)?)),
                "source" => page.sources.push(value.into()),
                "part" => {
                    let w: Vec<&str> = value.split_whitespace().collect();
                    let ch = w.first().and_then(|s| s.chars().next()).ok_or_else(|| err("part needs a character".into()))?;
                    let (k, used) = kind(&w[1..]).map_err(err)?;
                    let facing = dir(w.get(1 + used).copied().unwrap_or("W")).map_err(err)?;
                    legend.push((ch, k, facing));
                }
                _ => return Err(err(format!("unknown key {key:?}"))),
            }
        }
        if let Some(rows) = grid {
            let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
            page.board = Some(Board::parse(&refs, &legend).map_err(|e| format!("{id}: {e}"))?);
        }
        if page.title.is_empty() || page.blocks.is_empty() || page.sources.is_empty() {
            return Err(format!("{id}: a page needs a title, text and a source"));
        }
        Ok(page)
    }

    pub fn all() -> Vec<Page> {
        PAGES.iter().map(|(id, text)| Page::parse(id, text).unwrap_or_else(|e| panic!("page {id} doesn't parse: {e}"))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::Live;
    use crate::job::{JOBS, Job};

    #[test]
    fn every_page_parses_names_a_source_and_unlocks_with_a_real_job() {
        let jobs: Vec<String> = (0..JOBS.len()).map(|i| Job::load(i).id).collect();
        for p in Page::all() {
            assert!(!p.sources.is_empty(), "{} has no source", p.id);
            if let Some(u) = &p.unlock {
                assert!(jobs.contains(u), "{} unlocks with {u}, which isn't a job", p.id);
            }
        }
    }

    #[test]
    fn every_example_circuit_runs_without_burning() {
        for p in Page::all() {
            let Some(b) = &p.board else { continue };
            let mut live = Live::build(b);
            for _ in 0..500 {
                live.sim.step(0.001).unwrap();
            }
            for i in 0..b.parts.len() {
                assert!(!live.reading(b, i).burnt, "{}: {} burnt", p.id, b.parts[i].label);
            }
        }
    }

    #[test]
    fn the_worked_examples_match_the_solver() {
        // Ohm's law: 4.5 V across 220 ohm, about 20 mA (the battery's 0.45 ohm takes a little).
        let p = Page::all().into_iter().find(|p| p.id == "ohms-law").unwrap();
        let b = p.board.unwrap();
        let mut live = Live::build(&b);
        live.sim.solve_dc().unwrap();
        let r = live.reading(&b, b.part("R1").unwrap());
        assert!((r.amps - 0.0204).abs() < 0.0002, "{r:?}");
        // Power: 22 ohm on 12 V with a 20 ohm lamp: about 6 V across the resistor, about 1.6 W.
        let p = Page::all().into_iter().find(|p| p.id == "power").unwrap();
        let b = p.board.unwrap();
        let mut live = Live::build(&b);
        live.sim.solve_dc().unwrap();
        let r = live.reading(&b, b.part("R1").unwrap());
        assert!((r.volts - 6.28).abs() < 0.1 && (r.watts - 1.8).abs() < 0.2, "{r:?}");
        // The pages with a test point read what they say, and the fuse page's lamp draws its
        // 1.5 A through the fuse.
        let reads = |id: &str, label: &str| {
            let p = Page::all().into_iter().find(|p| p.id == id).unwrap();
            let b = p.board.unwrap();
            let mut live = Live::build(&b);
            live.sim.solve_dc().unwrap();
            live.reading(&b, b.part(label).unwrap())
        };
        // Dividers: 2.2k over 1k from 4.5 V, 4.5 x 1 / 3.2 = 1.41 V.
        assert!((reads("dividers", "TP1").volts - 1.41).abs() < 0.01);
        // Potentiometers: half way round, half the battery.
        assert!((reads("potentiometers", "TP1").volts - 2.25).abs() < 0.01);
        // Sensors: the LDR at 100 lux (about 3 kOhm) over 4.7k reads above half.
        let v = reads("sensors", "TP1").volts;
        assert!(v > 2.5 && v < 3.0, "{v}");
        let i = reads("fuses", "LP1").amps;
        assert!((i - 1.47).abs() < 0.02, "{i}");
        // Chapter 3 runs in time: each example as its page tells it.
        let run = |id: &str, set: &[(&str, bool)], secs: f64, label: &str| {
            let p = Page::all().into_iter().find(|p| p.id == id).unwrap();
            let mut b = p.board.unwrap();
            for (l, on) in set {
                let i = b.part(l).unwrap();
                b.parts[i].kind = crate::circuit::Kind::Switch { closed: *on };
            }
            let mut live = Live::build(&b);
            for _ in 0..(secs / 0.001f64).round() as usize {
                live.sim.step(0.001).unwrap();
            }
            (live, b, label.to_string())
        };
        let reading = |(live, b, label): (Live, crate::circuit::Board, String)| live.reading(&b, b.part(&label).unwrap());
        // Capacitors: switched on, the LED is lit; the capacitor holds about 4.5 V less the
        // battery's own drop.
        assert!(reading(run("capacitors", &[], 1.0, "LED1")).amps > 0.008);
        // The time constant: 10k and 100 uF is 1 s; after it, 63% of the 4.5 V.
        let v = reading(run("time-constant", &[("SW1", true)], 1.0, "TP1")).volts;
        assert!((v - 4.5 * 0.632).abs() < 0.05, "{v}");
        // Discharge: 10k in, 33k across, stops at 3.45 V.
        let v = reading(run("discharge", &[], 6.0, "TP1")).volts;
        assert!((v - 3.45).abs() < 0.02, "{v}");
        // Chapter 4: the relay lights its lamp once SW1 is on, and the NC contact is a NOT.
        assert!((reading(run("relays", &[("SW1", true)], 0.5, "LP1")).amps - 0.416).abs() < 0.005);
        assert!(reading(run("relays", &[], 0.5, "LP1")).amps < 1e-6);
        assert!(reading(run("relay-logic", &[], 0.5, "LED1")).amps > 0.010);
        assert!(reading(run("relay-logic", &[("SW1", true)], 0.5, "LED1")).amps < 1e-6);
        // Self-holding: START pressed and let go, the lamp stays on; STOP puts it out.
        let p = Page::all().into_iter().find(|p| p.id == "self-holding").unwrap();
        let mut b = p.board.unwrap();
        let mut live = Live::build(&b);
        let press = |b: &mut crate::circuit::Board, live: &mut Live, label: &str, held: bool| {
            let i = b.part(label).unwrap();
            let crate::circuit::Kind::Button { nc, .. } = b.parts[i].kind else { panic!() };
            b.parts[i].kind = crate::circuit::Kind::Button { held, nc };
            live.set_switch(i, held != nc);
            for _ in 0..300 {
                live.sim.step(0.001).unwrap();
            }
        };
        press(&mut b, &mut live, "PB1", true);
        press(&mut b, &mut live, "PB1", false);
        assert!(live.reading(&b, b.part("LP1").unwrap()).amps > 0.4);
        press(&mut b, &mut live, "PB2", true);
        press(&mut b, &mut live, "PB2", false);
        assert!(live.reading(&b, b.part("LP1").unwrap()).amps < 1e-6);
        // Buzzers: with PB1 held the relay chatters about 77 times a second.
        let p = Page::all().into_iter().find(|p| p.id == "buzzers").unwrap();
        let mut b = p.board.unwrap();
        let i = b.part("PB1").unwrap();
        b.parts[i].kind = crate::circuit::Kind::Button { held: true, nc: false };
        let mut live = Live::build(&b);
        let k = b.part("K1").unwrap();
        let (mut flips, mut was) = (0, false);
        for _ in 0..1000 {
            live.sim.step(0.001).unwrap();
            if live.energised(k) != was {
                flips += 1;
                was = !was;
            }
        }
        assert!((70..=80).contains(&(flips / 2)), "{flips}");
        // Chapter 5: 0.7 V across the diode; the transistor switches 14 mA; NOT; the dusk switch.
        let (live, b, _) = run("diodes", &[("SW1", true)], 0.5, "D1");
        let v = live.reading(&b, b.part("D1").unwrap()).volts;
        assert!((0.65..0.75).contains(&v), "{v}");
        assert!(live.reading(&b, b.part("LED1").unwrap()).amps > 0.010);
        let i = reading(run("transistors", &[("SW1", true)], 0.5, "LED1")).amps;
        assert!((i - 0.0146).abs() < 0.001, "{i}");
        assert!(reading(run("transistors", &[], 0.5, "LED1")).amps < 1e-6);
        assert!(reading(run("transistor-switches", &[], 0.5, "LED1")).amps < 1e-4);
        assert!(reading(run("transistor-switches", &[("SW1", false)], 0.5, "LED1")).amps > 0.010);
        assert!(reading(run("transistor-sensors", &[], 0.5, "LED1")).amps > 0.010);
        // Chapter 6: the blinker flashes about once a second; the one-shot stays green about 5 s.
        let b = Page::all().into_iter().find(|p| p.id == "blinkers").unwrap().board.unwrap();
        let mut live = Live::build(&b);
        let l = b.part("LED1").unwrap();
        let (mut rises, mut was) = (Vec::new(), false);
        for n in 0..6000 {
            live.sim.step(0.001).unwrap();
            let on = live.reading(&b, l).amps > 0.001;
            if on && !was {
                rises.push(n);
            }
            was = on;
        }
        let period = (rises[rises.len() - 1] - rises[1]) as f64 / (rises.len() - 2) as f64 / 1000.0;
        assert!((0.9..1.1).contains(&period), "{period} {rises:?}");
        let mut b = Page::all().into_iter().find(|p| p.id == "one-shots").unwrap().board.unwrap();
        let mut live = Live::build(&b);
        let (pb, green) = (b.part("PB1").unwrap(), b.part("LED2").unwrap());
        let mut lit = 0;
        for n in 0..9000 {
            if n == 1000 || n == 1200 {
                b.parts[pb].kind = crate::circuit::Kind::Button { held: n == 1000, nc: false };
                let mut new = Live::build(&b);
                new.keep_charge(&live);
                live = new;
            }
            live.sim.step(0.001).unwrap();
            if live.reading(&b, green).amps > 0.001 {
                lit += 1;
            }
        }
        assert!((4500..6000).contains(&lit), "{lit}");
    }
}
