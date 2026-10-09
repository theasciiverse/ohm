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
    }
}
