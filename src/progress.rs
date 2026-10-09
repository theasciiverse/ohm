//! Save data: the ledger, which jobs are solved, every job's board as you left it, and where
//! you dragged the panel edges. One plain text file, `$XDG_DATA_HOME/ohm/progress.txt` (or
//! `~/.local/share/ohm/progress.txt`), written after every test and on leaving.
//!
//! ```text
//! ledger 37.9
//! solved bike-lamp
//! best bike-lamp 37.9 thrifty frugal     (the best profit paid, and the ticks earned)
//! splits 30 - 12
//! open dolls-house                (the job on the desk when you left)
//! sound off                       (only when turned off)
//! board bike-lamp 48 26
//! drawer 2 1
//! copper 12 5 a                  (a tile, its mask as a hex digit, or x for a jumper)
//! part R1 16 5 W fixed=0 burnt=0 resistor 220 0.25
//! end
//! ```

use std::path::PathBuf;

use crate::circuit::{Board, Cell, Pos};
use crate::job::{Item, Ticks, dir, dir_name, kind, kind_words};
use crate::render::Splits;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    pub ledger: f64,
    pub solved: Vec<String>,
    /// Per job: the best profit paid so far, and the ticks earned.
    pub best: Vec<(String, f64, Ticks)>,
    pub splits: Splits,
    /// The job on the desk when last saved.
    pub open: Option<String>,
    /// Sound turned off from the menu or with m.
    pub muted: bool,
    /// Job id, its board, and how many of each drawer item are left.
    pub boards: Vec<(String, Board, Vec<Option<u32>>)>,
}

/// Set by the headless shots: they run the real game, autosave and all, and must never write
/// the player's save file.
pub static NO_SAVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("ohm").join("progress.txt"))
}

impl Progress {
    pub fn load() -> Progress {
        if cfg!(test) {
            return Progress::default();
        }
        let Some(p) = path() else { return Progress::default() };
        match std::fs::read_to_string(&p) {
            Ok(text) => Progress::parse(&text).unwrap_or_else(|e| {
                log::warn!("can't read {}: {e}", p.display());
                Progress::default()
            }),
            Err(_) => Progress::default(),
        }
    }

    pub fn save(&self) {
        if cfg!(test) || NO_SAVE.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            log::warn!("can't make {}: {e}", dir.display());
            return;
        }
        match std::fs::write(&p, self.to_text()) {
            Ok(()) => log::info!("saved {}", p.display()),
            Err(e) => log::warn!("can't save {}: {e}", p.display()),
        }
    }

    pub fn to_text(&self) -> String {
        let mut out = format!("ledger {}\n", self.ledger);
        for id in &self.solved {
            out += &format!("solved {id}\n");
        }
        for (id, profit, t) in &self.best {
            out += &format!("best {id} {profit}{}{}\n", if t.thrifty { " thrifty" } else { "" }, if t.frugal { " frugal" } else { "" });
        }
        if let Some(id) = &self.open {
            out += &format!("open {id}\n");
        }
        if self.muted {
            out += "sound off\n";
        }
        let opt = |v: Option<u16>| v.map(|n| n.to_string()).unwrap_or("-".into());
        out += &format!("splits {} {} {}\n", opt(self.splits.left), opt(self.splits.right), opt(self.splits.bottom));
        for (id, b, counts) in &self.boards {
            out += &format!("board {id} {} {}\n", b.w, b.h);
            for (i, c) in counts.iter().enumerate() {
                out += &format!("drawer {i} {}\n", c.map(|n| n.to_string()).unwrap_or("-".into()));
            }
            for y in 0..b.h {
                for x in 0..b.w {
                    match b.cell(Pos { x, y }) {
                        Cell::Trace(m) => out += &format!("copper {x} {y} {m:x}\n"),
                        Cell::Jumper => out += &format!("copper {x} {y} x\n"),
                        Cell::Blocked => out += &format!("copper {x} {y} #\n"),
                        _ => {}
                    }
                }
            }
            for p in &b.parts {
                out += &format!(
                    "part {} {} {} {} fixed={} burnt={} {}\n",
                    p.label,
                    p.pos.x,
                    p.pos.y,
                    dir_name(p.facing),
                    p.fixed as u8,
                    p.burnt as u8,
                    kind_words(p.kind)
                );
            }
            out += "end\n";
        }
        out
    }

    pub fn parse(text: &str) -> Result<Progress, String> {
        let mut p = Progress::default();
        let mut current: Option<(String, Board, Vec<Option<u32>>)> = None;
        for (n, line) in text.lines().enumerate() {
            let err = |e: &str| format!("line {}: {e}", n + 1);
            let w: Vec<&str> = line.split_whitespace().collect();
            let num = |i: usize| w.get(i).and_then(|s| s.parse::<usize>().ok()).ok_or_else(|| err("expected a number"));
            match w.first().copied() {
                None => {}
                Some("ledger") => p.ledger = w.get(1).and_then(|s| s.parse().ok()).ok_or_else(|| err("bad ledger"))?,
                Some("solved") => p.solved.push(w.get(1).ok_or_else(|| err("no job"))?.to_string()),
                Some("best") => {
                    let id = w.get(1).ok_or_else(|| err("no job"))?.to_string();
                    let profit = w.get(2).and_then(|s| s.parse().ok()).ok_or_else(|| err("bad profit"))?;
                    let t = Ticks { works: true, thrifty: w[3..].contains(&"thrifty"), frugal: w[3..].contains(&"frugal") };
                    p.best.push((id, profit, t));
                }
                Some("open") => p.open = Some(w.get(1).ok_or_else(|| err("no job"))?.to_string()),
                Some("sound") => p.muted = w.get(1) == Some(&"off"),
                Some("splits") => {
                    let get = |i: usize| w.get(i).and_then(|s| s.parse::<u16>().ok());
                    p.splits = Splits { left: get(1), right: get(2), bottom: get(3) };
                }
                Some("board") => {
                    let id = w.get(1).ok_or_else(|| err("no job"))?.to_string();
                    current = Some((id, Board::new(num(2)?, num(3)?), Vec::new()));
                }
                Some("drawer") => {
                    let (_, _, counts) = current.as_mut().ok_or_else(|| err("drawer outside a board"))?;
                    let i = num(1)?;
                    if counts.len() <= i {
                        counts.resize(i + 1, None);
                    }
                    counts[i] = w.get(2).and_then(|s| s.parse().ok());
                }
                Some("copper") => {
                    let (_, b, _) = current.as_mut().ok_or_else(|| err("copper outside a board"))?;
                    let at = Pos { x: num(1)?, y: num(2)? };
                    if at.x >= b.w || at.y >= b.h {
                        return Err(err("copper off the board"));
                    }
                    let c = match w.get(3).copied() {
                        Some("x") => Cell::Jumper,
                        Some("#") => Cell::Blocked,
                        Some(m) => Cell::Trace(u8::from_str_radix(m, 16).map_err(|_| err("bad mask"))?),
                        None => return Err(err("no mask")),
                    };
                    b.set_plain(at, c);
                }
                Some("part") => {
                    let (_, b, _) = current.as_mut().ok_or_else(|| err("part outside a board"))?;
                    let label = w.get(1).ok_or_else(|| err("no label"))?.to_string();
                    let at = Pos { x: num(2)?, y: num(3)? };
                    let facing = dir(w.get(4).copied().unwrap_or("W")).map_err(|e| err(&e))?;
                    let fixed = w.get(5) == Some(&"fixed=1");
                    let burnt = w.get(6) == Some(&"burnt=1");
                    let (k, _) = kind(&w[7..]).map_err(|e| err(&e))?;
                    if at.x >= b.w || at.y >= b.h {
                        return Err(err("part off the board"));
                    }
                    let i = b.place(k, at, facing, fixed).ok_or_else(|| err("tile taken"))?;
                    b.parts[i].label = label;
                    b.parts[i].burnt = burnt;
                }
                Some("end") => {
                    if let Some(done) = current.take() {
                        p.boards.push(done);
                    }
                }
                Some(other) => return Err(err(&format!("unknown {other:?}"))),
            }
        }
        Ok(p)
    }

    /// The saved board for a job, if its drawer still matches the job's.
    pub fn board_for(&self, id: &str, drawer: &[Item]) -> Option<(Board, Vec<Item>)> {
        let (_, b, counts) = self.boards.iter().find(|(i, _, _)| i == id)?;
        if counts.len() != drawer.len() {
            return None;
        }
        // Unlimited items stay unlimited; limited ones take the saved count.
        let items = drawer.iter().zip(counts).map(|(it, &c)| Item { count: if it.count.is_some() { c } else { None }, ..it.clone() }).collect();
        Some((b.clone(), items))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::{Colour, Dir, Kind};

    #[test]
    fn progress_round_trips() {
        let mut b = Board::new(12, 6);
        b.place(Kind::Battery { volts: 4.5, r_int: 0.45 }, Pos { x: 1, y: 2 }, Dir::N, true);
        b.place(Kind::Led(Colour::Green), Pos { x: 8, y: 2 }, Dir::S, false);
        b.place(Kind::Switch { closed: false }, Pos { x: 4, y: 1 }, Dir::W, false);
        b.connect(Pos { x: 1, y: 2 }, Dir::N);
        b.connect(Pos { x: 1, y: 1 }, Dir::E);
        b.set_plain(Pos { x: 6, y: 4 }, Cell::Jumper);
        b.parts[1].burnt = true;
        let p = Progress {
            ledger: 37.9,
            solved: vec!["bike-lamp".into()],
            best: vec![("bike-lamp".into(), 37.9, Ticks { works: true, thrifty: true, frugal: false })],
            splits: Splits { left: Some(30), right: None, bottom: Some(12) },
            open: Some("bike-lamp".into()),
            muted: true,
            boards: vec![("bike-lamp".into(), b.clone(), vec![Some(1), None, Some(0)]), ("sandbox".into(), b, Vec::new())],
        };
        let back = Progress::parse(&p.to_text()).unwrap();
        assert_eq!(back, p);
    }
}
