//! The game: one job on the desk, edited with mouse and keyboard, simulated live.
//!
//! The board always runs. Every edit rebuilds the running circuit from the board (so what is
//! drawn is what is simulated); flipping a switch or pressing a button changes the running
//! circuit in place. Time advances in 1 ms solver steps, as many per frame as the frame took.

use std::collections::VecDeque;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::audio::{Audio, Sfx};
use crate::bench::{diagnose, run_case, si};
use crate::circuit::{Board, Cell, Dir, Kind, Live, Pos};
use crate::input::Act;
use crate::job::{Item, Job, Score, Ticks, same_part};
use crate::book::Page;
use crate::progress::Progress;
use crate::render::{self, BoardView, Card, DeskView, Edge, Layout, Meter, Report, Slot, Splits};

/// One solver step, seconds.
const STEP: f64 = 0.001;
/// The workbench: one big perfboard, the job's board laid in its middle.
const BENCH: (usize, usize) = (48, 26);
/// Seconds after the last edit before it's saved.
const AUTOSAVE: f64 = 2.0;
/// How much history the scope shows, in frames' worth of samples.
const SCOPE_LEN: usize = 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Copper,
    Item(usize),
    /// The eraser: clicks and drags remove copper and parts.
    Eraser,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// The launch page.
    Title,
    Desk,
}

/// What the launch page's menu offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItem {
    Continue,
    Notebook,
    Sound,
    Quit,
}

pub enum Overlay {
    None,
    Drawers { sel: usize, scroll: usize },
    Report(Report),
    /// The Master's Notebook open at a page, its text scrolled down by `scroll` lines.
    Book { page: usize, scroll: usize },
}

/// A mouse stroke in progress on the board.
struct Stroke {
    last: Pos,
    moved: bool,
    right: bool,
    before: (Board, Vec<Item>),
}

pub struct App {
    pub screen: Screen,
    /// The launch page's highlighted entry.
    pub title_sel: usize,
    /// Every job, in order; `job` is a copy of the open one.
    pub jobs: Vec<Job>,
    pub current: usize,
    /// Each job's board and drawers as last left, while another job is open.
    saved: Vec<Option<(Board, Vec<Item>)>>,
    pub solved: Vec<bool>,
    /// Per job: the best profit paid so far and the ticks earned, once it has passed.
    best: Vec<Option<(f64, Ticks)>>,
    /// Per job: the old master's score, the par to beat.
    pars: Vec<Score>,
    /// There was a save to pick up, or work has been done since: the menu says Continue.
    returning: bool,
    /// The Master's Notebook, and the open page's example circuit, running.
    pub pages: Vec<Page>,
    example: Option<(usize, Board, Live)>,
    /// The example's tile under the pointer: the meter, scope and card read it.
    book_point: Option<Pos>,
    pub job: Job,
    pub board: Board,
    /// What's left in the drawers.
    pub drawer: Vec<Item>,
    live: Live,
    flow: Vec<[f64; 4]>,
    pub cursor: Pos,
    pub pick: Pick,
    /// Which way the next part placed faces.
    pub facing: Dir,
    pub copper_mode: bool,
    /// Where the scope's probe is pinned; without one the scope watches the cursor.
    pub probe: Option<Pos>,
    /// The next board click pins the probe (after the PROBE button).
    probe_armed: bool,
    undo: Vec<(Board, Vec<Item>)>,
    redo: Vec<(Board, Vec<Item>)>,
    pub overlay: Overlay,
    pub time: f64,
    pub ledger: f64,
    scope: VecDeque<f64>,
    /// What the scope watched last: on the Notebook's example or not, and where.
    scope_target: Option<(bool, Pos)>,
    stroke: Option<Stroke>,
    /// A button held by the keyboard until this time; by the mouse until it lets go.
    held: Option<(usize, Option<f64>)>,
    message: Option<(String, f64)>,
    /// Sound as the player last set it, for the save file (`--mute` alone doesn't stick).
    sound_off: bool,
    /// The board and drawers as last seen, and when they last changed: a save follows a
    /// couple of seconds after the last edit.
    seen: (Board, Vec<Item>),
    changed_at: Option<f64>,
    /// The circuit couldn't be solved (two sources fighting).
    unsolvable: bool,
    pub area: Rect,
    /// Where the player has dragged the panel edges.
    pub splits: Splits,
    /// The edge being dragged, and the one under the mouse.
    resizing: Option<Edge>,
    /// The tile the board panel is centred on.
    pub view: (usize, usize),
    /// Where the job's own board lies on the workbench.
    pub job_at: Pos,
    /// A middle-button drag in progress: where it started, the view then, and whether it moved.
    pan: Option<(u16, u16, (usize, usize), bool)>,
    hover_edge: Option<Edge>,
    pub quit: bool,
    pub audio: Audio,
    acc: f64,
}

/// Two kinds are the same part from the drawer, whatever a switch's position.
/// Which way a part faces when dropped on copper with mask `m`: in line with straight copper (or
/// a stub), keeping the player's chosen direction when it already fits. `None` on a corner or a
/// junction.
fn in_line(m: u8, kind: Kind, wanted: Dir) -> Option<Dir> {
    let ew = Dir::E.bit() | Dir::W.bit();
    let ns = Dir::N.bit() | Dir::S.bit();
    if kind.pins() == 1 {
        // A test point faces the copper it meets.
        return Dir::ALL.into_iter().find(|d| m & d.bit() != 0).filter(|_| m.count_ones() == 1);
    }
    if m & ns == 0 {
        Some(if matches!(wanted, Dir::E | Dir::W) { wanted } else { Dir::W })
    } else if m & ew == 0 {
        Some(if matches!(wanted, Dir::N | Dir::S) { wanted } else { Dir::N })
    } else {
        None
    }
}

fn rotate(d: Dir) -> Dir {
    match d {
        Dir::N => Dir::E,
        Dir::E => Dir::S,
        Dir::S => Dir::W,
        Dir::W => Dir::N,
    }
}

pub fn part_name(k: Kind) -> String {
    match k {
        Kind::Resistor { ohms, watts } => {
            let value = si(ohms, "Ω").replace(".00 ", " ").replace(".0 ", " ");
            if watts == 0.25 { format!("Resistor {value}") } else { format!("Resistor {value} {watts} W") }
        }
        Kind::Lamp { .. } => "Lamp".into(),
        Kind::Battery { volts, .. } => format!("Battery {}", si(volts, "V")),
        Kind::Supply { .. } => "Bench supply".into(),
        Kind::Switch { .. } => "Slide switch".into(),
        Kind::Button { .. } => "Push button".into(),
        Kind::Led(c) => format!("LED, {}", c.name()),
        Kind::Capacitor { farads } => format!("Capacitor {}", si(farads, "F")),
        Kind::TestPoint => "Test point".into(),
    }
}

impl App {
    /// The desk with every job, picking up where `progress` left off, on the launch page.
    pub fn new(jobs: Vec<Job>, progress: Progress, audio: Audio) -> App {
        let solved: Vec<bool> = jobs.iter().map(|j| progress.solved.contains(&j.id)).collect();
        let best = jobs.iter().map(|j| progress.best.iter().find(|b| b.0 == j.id).map(|b| (b.1, b.2))).collect();
        let pars = jobs.iter().map(Job::par).collect();
        let saved: Vec<Option<(Board, Vec<Item>)>> = jobs.iter().map(|j| progress.board_for(&j.id, &j.drawer)).collect();
        let job = jobs[0].clone();
        let (board, job_at) = job.board.embed(BENCH.0, BENCH.1);
        let live = Live::build(&board);
        let drawer = job.drawer.clone();
        let cursor = Pos { x: job_at.x + job.board.w / 2, y: job_at.y + job.board.h / 2 };
        let mut app = App {
            screen: Screen::Title,
            title_sel: 0,
            jobs,
            current: 0,
            returning: !progress.boards.is_empty() || !progress.solved.is_empty(),
            saved,
            solved,
            best,
            pars,
            pages: Page::all(),
            example: None,
            book_point: None,
            job,
            flow: vec![[0.0; 4]; board.w * board.h],
            board,
            drawer,
            live,
            cursor,
            pick: Pick::Copper,
            facing: Dir::W,
            copper_mode: false,
            probe: None,
            probe_armed: false,
            undo: Vec::new(),
            redo: Vec::new(),
            overlay: Overlay::None,
            time: 0.0,
            ledger: progress.ledger,
            scope: VecDeque::new(),
            scope_target: None,
            stroke: None,
            held: None,
            message: None,
            unsolvable: false,
            sound_off: progress.muted,
            seen: (Board::new(1, 1), Vec::new()),
            changed_at: None,
            area: Rect::new(0, 0, render::MIN_W, render::MIN_H),
            splits: progress.splits,
            resizing: None,
            view: (cursor.x, cursor.y),
            job_at,
            pan: None,
            hover_edge: None,
            quit: false,
            audio,
            acc: 0.0,
        };
        // Open the job that was on the desk, else the first not yet solved, as it was left.
        let first = (0..app.jobs.len()).find(|&i| !app.solved[i]).unwrap_or(0);
        let open = progress.open.as_ref().and_then(|id| app.jobs.iter().position(|j| &j.id == id)).filter(|&i| app.unlocked(i));
        app.saved_into_place(open.unwrap_or(first));
        app.seen = app.snapshot();
        app
    }

    /// A page is open from the start, or once the job it belongs to has reached the inbox.
    pub fn page_unlocked(&self, i: usize) -> bool {
        match &self.pages[i].unlock {
            None => true,
            Some(id) => self.jobs.iter().position(|j| &j.id == id).is_some_and(|k| self.unlocked(k)),
        }
    }

    fn open_page(&mut self, i: usize) {
        if i >= self.pages.len() || !self.page_unlocked(i) {
            self.audio.play(Sfx::Denied);
            return;
        }
        self.example = self.pages[i].board.as_ref().map(|b| (i, b.clone(), Live::build(b)));
        self.book_point = None;
        self.overlay = Overlay::Book { page: i, scroll: 0 };
        log::info!("notebook page {}", self.pages[i].id);
        self.audio.play(Sfx::Select);
    }

    fn done(&self) -> bool {
        self.solved[self.current]
    }

    /// A job can be opened once the one before it is solved.
    pub fn unlocked(&self, i: usize) -> bool {
        i == 0 || self.solved.get(i - 1).copied().unwrap_or(false)
    }

    /// Opens job `i`: puts the open one's board aside, and brings this one's back as it was
    /// left, or fresh.
    pub fn open_job(&mut self, i: usize) {
        if i >= self.jobs.len() || i == self.current {
            return;
        }
        if !self.unlocked(i) {
            self.audio.play(Sfx::Denied);
            self.say(format!("Finish {} first.", self.jobs[i - 1].name));
            return;
        }
        self.saved[self.current] = Some(self.snapshot());
        self.saved_into_place(i);
        self.audio.play(Sfx::Select);
        self.save();
    }

    fn saved_into_place(&mut self, i: usize) {
        self.current = i;
        self.job = self.jobs[i].clone();
        let (fresh, job_at) = self.job.board.embed(BENCH.0, BENCH.1);
        (self.board, self.drawer) = self.saved[i].take().unwrap_or((fresh, self.job.drawer.clone()));
        self.job_at = job_at;
        self.cursor = Pos { x: job_at.x + self.job.board.w / 2, y: job_at.y + self.job.board.h / 2 };
        self.view = (self.cursor.x, self.cursor.y);
        self.undo.clear();
        self.redo.clear();
        self.probe = None;
        self.pick = Pick::Copper;
        self.overlay = Overlay::None;
        self.rebuilt();
        log::info!("job {} ({}) on the desk", self.job.id, self.job.name);
    }

    /// Writes the ledger, the solved jobs, every board and the panel edges to the save file.
    pub fn save(&self) {
        let mut boards = Vec::new();
        for (i, job) in self.jobs.iter().enumerate() {
            let state = if i == self.current { Some((&self.board, &self.drawer)) } else { self.saved[i].as_ref().map(|(b, d)| (b, d)) };
            if let Some((b, d)) = state {
                boards.push((job.id.clone(), b.clone(), d.iter().map(|it| it.count).collect()));
            }
        }
        let solved = self.jobs.iter().zip(&self.solved).filter(|(_, s)| **s).map(|(j, _)| j.id.clone()).collect();
        let best = self.jobs.iter().zip(&self.best).filter_map(|(j, b)| b.map(|(profit, t)| (j.id.clone(), profit, t))).collect();
        let open = Some(self.job.id.clone());
        Progress { ledger: self.ledger, solved, best, splits: self.splits, open, muted: self.sound_off, boards }.save();
    }

    fn layout(&self) -> Option<Layout> {
        render::layout(self.area, &self.board, self.drawer.len() + 1, self.splits, self.view)
    }

    /// A tile of the job's own board, on the workbench.
    pub fn at(&self, x: usize, y: usize) -> Pos {
        Pos { x: self.job_at.x + x, y: self.job_at.y + y }
    }

    /// Moves the view so the cursor stays in sight, with a tile to spare where there's room.
    fn follow(&mut self) {
        let Some(l) = self.layout() else { return };
        let (cols, rows) = render::tiles_shown(&l);
        let (vx, vy) = l.view;
        let c = self.cursor;
        if c.x < vx + 1 || c.x + 2 > vx + cols {
            self.view.0 = c.x;
        }
        if c.y < vy + 1 || c.y + 2 > vy + rows {
            self.view.1 = c.y;
        }
    }

    fn say(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        log::info!("message: {msg}");
        self.message = Some((msg, self.time + 3.0));
    }

    // ------------------------------------------------------------ editing

    fn snapshot(&self) -> (Board, Vec<Item>) {
        (self.board.clone(), self.drawer.clone())
    }

    fn remember(&mut self) {
        self.undo.push(self.snapshot());
        self.redo.clear();
    }

    /// Rebuilds the running circuit after the board changed.
    fn rebuilt(&mut self) {
        self.live = Live::build(&self.board);
        self.unsolvable = false;
        self.flow = vec![[0.0; 4]; self.board.w * self.board.h];
    }

    fn place(&mut self, at: Pos) {
        let i = match self.pick {
            Pick::Item(i) => i,
            Pick::Eraser => {
                self.erase(at);
                return;
            }
            Pick::Copper => {
                self.say("Copper is laid by moving: shift+arrows, or drag with the mouse.");
                return;
            }
        };
        let kind = self.drawer[i].kind;
        // On straight copper the part goes in line with it, splitting the trace; anywhere but an
        // empty hole or straight copper is taken.
        let facing = match self.board.cell(at) {
            Cell::Empty => self.facing,
            Cell::Trace(m) => match in_line(m, kind, self.facing) {
                Some(f) => f,
                None => {
                    self.audio.play(Sfx::Denied);
                    self.say("Parts go on straight copper, not on a corner or a junction.");
                    return;
                }
            },
            _ => {
                self.audio.play(Sfx::Denied);
                self.say("That tile is taken. Erase it first (x, or right-click).");
                return;
            }
        };
        if self.drawer[i].count == Some(0) {
            self.audio.play(Sfx::Denied);
            self.say(format!("The drawer is out of {}.", part_name(self.drawer[i].kind)));
            return;
        }
        self.remember();
        if matches!(self.board.cell(at), Cell::Trace(_)) {
            self.board.erase(at);
        }
        if let Some(n) = self.board.place(kind, at, facing, false) {
            if let Some(c) = &mut self.drawer[i].count {
                *c -= 1;
            }
            log::info!("placed {} at {},{} facing {:?}", self.board.parts[n].label, at.x, at.y, facing);
            self.audio.play(Sfx::Place);
            self.rebuilt();
        }
    }

    fn erase(&mut self, at: Pos) -> bool {
        let part = match self.board.cell(at) {
            Cell::Part(i) => Some(self.board.parts[i].clone()),
            Cell::Empty | Cell::Blocked => return false,
            _ => None,
        };
        if part.as_ref().is_some_and(|p| p.fixed) {
            self.audio.play(Sfx::Denied);
            self.say("That part belongs to the gadget: it stays.");
            return false;
        }
        let before = self.snapshot();
        if !self.board.erase(at) {
            return false;
        }
        if let Some(p) = part {
            if let Some(item) = self.drawer.iter_mut().find(|it| same_part(it.kind, p.kind))
                && let Some(c) = &mut item.count
            {
                *c += 1;
            }
            log::info!("erased {}", p.label);
        }
        self.undo.push(before);
        self.redo.clear();
        self.audio.play(Sfx::Erase);
        self.rebuilt();
        true
    }

    /// Lays copper from `from` toward `d`; true if any was laid.
    fn lay(&mut self, from: Pos, d: Dir) -> bool {
        let before = self.board.clone();
        if self.board.connect(from, d) {
            if self.board != before {
                self.audio.play(Sfx::Lay);
                self.rebuilt();
            }
            true
        } else {
            false
        }
    }

    fn rotate_at(&mut self, at: Pos) {
        match self.board.cell(at) {
            Cell::Part(i) if self.board.parts[i].fixed => {
                self.audio.play(Sfx::Denied);
                self.say("That part belongs to the gadget: it stays as it is.");
            }
            Cell::Part(i) => {
                self.remember();
                let p = &mut self.board.parts[i];
                p.facing = rotate(p.facing);
                log::info!("rotated {} to {:?}", p.label, p.facing);
                self.audio.play(Sfx::Place);
                self.rebuilt();
            }
            _ => {
                self.facing = rotate(self.facing);
                self.audio.play(Sfx::Select);
            }
        }
    }

    fn operate(&mut self, at: Pos) {
        match self.board.cell(at) {
            Cell::Part(i) => match self.board.parts[i].kind {
                Kind::Switch { closed } => {
                    self.board.parts[i].kind = Kind::Switch { closed: !closed };
                    self.live.set_switch(i, !closed);
                    log::info!("{} {}", self.board.parts[i].label, if closed { "off" } else { "on" });
                    self.audio.play(Sfx::Click);
                }
                Kind::Button { .. } => self.hold(i, Some(self.time + 0.4)),
                _ => {}
            },
            Cell::Trace(0b1111) | Cell::Jumper => {
                self.remember();
                self.board.toggle_jumper(at);
                self.audio.play(Sfx::Place);
                self.rebuilt();
            }
            _ => {}
        }
    }

    fn hold(&mut self, part: usize, until: Option<f64>) {
        self.board.parts[part].kind = Kind::Button { held: true };
        self.live.set_switch(part, true);
        self.held = Some((part, until));
        self.audio.play(Sfx::Click);
    }

    fn release(&mut self) {
        if let Some((part, _)) = self.held.take()
            && part < self.board.parts.len()
        {
            self.board.parts[part].kind = Kind::Button { held: false };
            self.live.set_switch(part, false);
            self.audio.play(Sfx::Click);
        }
    }

    fn mend(&mut self) {
        if self.board.parts.iter().any(|p| p.burnt) {
            self.remember();
            for p in &mut self.board.parts {
                p.burnt = false;
            }
            self.say("Burnt parts replaced.");
            self.audio.play(Sfx::Place);
            self.rebuilt();
        } else {
            self.say("Nothing is burnt.");
        }
    }

    fn step_undo(&mut self, back: bool) {
        let (from, to) = if back { (&mut self.undo, &mut self.redo) } else { (&mut self.redo, &mut self.undo) };
        if let Some(state) = from.pop() {
            to.push((self.board.clone(), self.drawer.clone()));
            (self.board, self.drawer) = state;
            self.audio.play(Sfx::Erase);
            self.rebuilt();
        }
    }

    fn test(&mut self) {
        let rows: Vec<(String, bool, String)> = self
            .job
            .cases
            .iter()
            .map(|c| {
                let o = run_case(&self.board, c);
                (c.name.clone(), o.pass, o.why)
            })
            .collect();
        let pass = rows.iter().all(|r| r.1);
        let mine = self.job.score(&self.board);
        let ticks = self.job.ticks(pass, mine);
        let mut summary = Vec::new();
        let mut hint = None;
        if pass {
            let i = self.current;
            let fee = self.job.fee;
            let profit = fee - mine.cost;
            let who = self.job.customer.split(',').next().unwrap_or_default().to_string();
            let before = self.best[i];
            match before {
                None if !self.solved[i] => {
                    self.ledger += profit;
                    summary.push(format!("{who} pays {fee:.2} kr; parts {:.2} kr; profit {profit:.2} kr.", mine.cost));
                }
                Some((best, _)) if profit > best + 0.005 => {
                    self.ledger += profit - best;
                    summary.push(format!("Cheaper than your last build: {:.2} kr more profit goes in the ledger.", profit - best));
                }
                _ => summary.push(format!("{who} has paid for this job. A cheaper build earns the difference.")),
            }
            // The battery tick earns a tip, once.
            if self.job.life.is_some() && ticks.frugal && !before.is_some_and(|b| b.1.frugal) {
                let tip = (fee / 10.0).round();
                self.ledger += tip;
                summary.push(format!("{who} tips {tip:.0} kr: the battery will last."));
            }
            let first_time = !self.solved[i];
            self.solved[i] = true;
            self.best[i] = Some(match before {
                Some((best, t)) => (best.max(profit), t.or(ticks)),
                None => (profit, ticks),
            });
            if first_time && let Some(next) = self.jobs.get(i + 1) {
                summary.push(format!("A new job is in the inbox: {}. Click it, or press PgDn.", next.name));
            }
            self.audio.play(Sfx::Pass);
        } else {
            if let Some(h) = diagnose(&self.board).into_iter().next() {
                hint = Some([h.nudge, h.detail, h.fix]);
            }
            self.audio.play(Sfx::Fail);
        }
        let goals = self.goals(self.current, Some((mine, ticks)));
        log::info!("test bench: {}", if pass { "pass" } else { "fail" });
        self.save();
        self.overlay = Overlay::Report(Report { pass, rows, goals, summary, hint });
    }

    /// The ticks a job has earned so far, one per goal it has (no battery tick where the
    /// battery doesn't matter).
    fn marks(&self, i: usize) -> Vec<bool> {
        let t = self.best[i].map(|b| b.1).unwrap_or(Ticks { works: self.solved[i], ..Ticks::default() });
        let mut m = vec![t.works, t.thrifty];
        if self.jobs[i].life.is_some() {
            m.push(t.frugal);
        }
        m
    }

    /// A job's goals, each with whether it's earned. With a build's score (after a test), the
    /// lines compare it with the master's.
    fn goals(&self, i: usize, test: Option<(Score, Ticks)>) -> Vec<(String, bool)> {
        let par = self.pars[i];
        let earned = self.marks(i);
        let now = |k: usize| test.map(|(_, t)| [t.works, t.thrifty, t.frugal][k]).unwrap_or(earned[k]);
        let mut g = vec![("It works: every line of the spec passes".to_string(), now(0))];
        g.push(match test {
            Some((mine, _)) => (format!("Parts: yours {:.2} kr, the master's {:.2} kr", mine.cost, par.cost), now(1)),
            None => (format!("parts max {:.2} kr", par.cost), now(1)),
        });
        if let Some(h) = par.hours {
            g.push(match test.map(|(m, _)| m.hours) {
                Some(Some(mine)) => (format!("Battery: yours lasts {mine:.0} h, the master's {h:.0} h", ), now(2)),
                _ => (format!("battery min {h:.0} h"), now(2)),
            });
        }
        g
    }

    fn toggle_sound(&mut self) {
        self.audio.toggle_mute();
        self.sound_off = self.audio.muted;
        self.save();
    }

    /// Saves a couple of seconds after the last change to the board, so a closed window or a
    /// crash loses at most that.
    fn autosave(&mut self) {
        if self.board != self.seen.0 || self.drawer != self.seen.1 {
            self.seen = self.snapshot();
            self.changed_at = Some(self.time);
        }
        if let Some(t) = self.changed_at
            && self.time - t >= AUTOSAVE
        {
            self.changed_at = None;
            log::info!("autosave");
            self.save();
        }
    }

    fn supply_amps(&self) -> f64 {
        (0..self.board.parts.len())
            .filter(|&i| self.board.parts[i].kind.is_source())
            .map(|i| self.live.reading(&self.board, i).amps.max(0.0))
            .sum()
    }

    fn pin_probe(&mut self, p: Pos) {
        if self.probe == Some(p) {
            self.probe = None;
            self.say("Probe off: the scope follows the cursor again.");
        } else {
            self.probe = Some(p);
            self.say("Probe clipped on: the scope watches it wherever the cursor goes. p or PROBE takes it off.");
        }
        self.audio.play(Sfx::Click);
    }

    fn slots(&self) -> usize {
        self.drawer.len() + 1
    }

    fn pick_slot(&mut self, n: usize) {
        if n < self.slots() {
            self.pick = if n == 0 { Pick::Copper } else { Pick::Item(n - 1) };
            self.audio.play(Sfx::Select);
        }
    }

    fn slot_index(&self) -> usize {
        match self.pick {
            Pick::Copper | Pick::Eraser => 0,
            Pick::Item(i) => i + 1,
        }
    }

    fn step_cursor(&mut self, d: Dir) {
        if let Some(q) = self.board.neighbour(self.cursor, d) {
            self.cursor = q;
            self.follow();
        }
    }

    // ------------------------------------------------------------ acts

    pub fn act(&mut self, a: Act) {
        if !matches!(a, Act::Hover(..) | Act::Drag(..)) {
            log::debug!("act {a:?}");
        }
        if matches!(a, Act::Quit) {
            self.quit = true;
            return;
        }
        if self.screen == Screen::Title {
            self.title_act(a);
            return;
        }
        match &mut self.overlay {
            Overlay::Report(_) => {
                if matches!(a, Act::Place | Act::Back | Act::Test | Act::Down(..)) {
                    self.overlay = Overlay::None;
                }
                return;
            }
            Overlay::Drawers { .. } => {
                self.drawer_act(a);
                return;
            }
            Overlay::Book { .. } => {
                self.book_act(a);
                return;
            }
            Overlay::None => {}
        }
        match a {
            Act::Move(d) => {
                if self.copper_mode {
                    let from = self.cursor;
                    self.remember();
                    if !self.lay(from, d) {
                        self.undo.pop();
                    }
                }
                self.step_cursor(d);
            }
            Act::Lay(d) => {
                let from = self.cursor;
                self.remember();
                if !self.lay(from, d) {
                    self.undo.pop();
                    self.audio.play(Sfx::Denied);
                }
                self.step_cursor(d);
            }
            Act::Place => self.place(self.cursor),
            Act::Rotate => self.rotate_at(self.cursor),
            Act::Erase => {
                self.erase(self.cursor);
            }
            Act::Operate => self.operate(self.cursor),
            Act::Test => self.test(),
            Act::Drawer => {
                self.overlay = Overlay::Drawers { sel: self.slot_index(), scroll: 0 };
                self.audio.play(Sfx::Select);
            }
            Act::Notebook => {
                let last = match &self.overlay {
                    Overlay::Book { page, .. } => *page,
                    _ => 0,
                };
                self.open_page(last);
            }
            Act::Probe => {
                if self.probe.is_some() {
                    self.probe = None;
                    self.say("Probe off: the scope follows the cursor again.");
                    self.audio.play(Sfx::Select);
                } else {
                    self.pin_probe(self.cursor);
                }
            }
            Act::MiddleDown(x, y) => self.pan = Some((x, y, self.view, false)),
            Act::MiddleDrag(x, y) => {
                if let Some((sx, sy, (cx, cy), _)) = self.pan {
                    let shift = |from: usize, start: u16, now: u16, tile: u16, max: usize| {
                        let d = (start as i32 - now as i32) / tile as i32;
                        (from as i32 + d).clamp(0, max as i32 - 1) as usize
                    };
                    self.view = (shift(cx, sx, x, render::TILE_W, self.board.w), shift(cy, sy, y, render::TILE_H, self.board.h));
                    self.pan = Some((sx, sy, (cx, cy), true));
                }
            }
            Act::Slot(n) => self.pick_slot(n),
            Act::NextSlot => self.pick_slot((self.slot_index() + 1) % self.slots()),
            Act::PrevSlot => self.pick_slot((self.slot_index() + self.slots() - 1) % self.slots()),
            Act::CopperMode => {
                self.copper_mode = !self.copper_mode;
                self.say(if self.copper_mode { "Laying copper as you move. c again to stop." } else { "Moving without laying copper." });
            }
            Act::Eraser => {
                if self.pick == Pick::Eraser {
                    self.pick = Pick::Copper;
                    self.say("Eraser put down.");
                } else {
                    self.pick = Pick::Eraser;
                    self.say("Eraser in hand: click or drag over copper and parts to remove them. Esc puts it down.");
                }
                self.audio.play(Sfx::Select);
            }
            Act::Mend => self.mend(),
            Act::Undo => self.step_undo(true),
            Act::Redo => self.step_undo(false),
            Act::Mute => self.toggle_sound(),
            Act::Back => {
                // Esc puts down whatever is in hand; with nothing in hand it goes to the launch page.
                if self.copper_mode || self.probe_armed || self.pick == Pick::Eraser {
                    self.copper_mode = false;
                    self.probe_armed = false;
                    if self.pick == Pick::Eraser {
                        self.pick = Pick::Copper;
                    }
                } else {
                    self.screen = Screen::Title;
                    self.save();
                }
            }
            Act::NextJob => self.open_job(self.current + 1),
            Act::PrevJob => {
                if self.current > 0 {
                    self.open_job(self.current - 1);
                }
            }
            Act::Quit => {}
            Act::Hover(x, y) => {
                if let Some(l) = self.layout() {
                    self.hover_edge = render::edge_at(&l, x, y);
                    if let Some(p) = render::tile_at(&l, &self.board, x, y) {
                        self.cursor = p;
                    }
                }
            }
            Act::Down(x, y, right) => self.mouse_down(x, y, right),
            Act::Drag(x, y, right) => self.mouse_drag(x, y, right),
            Act::Up(x, y) => self.mouse_up(x, y),
            Act::Scroll(x, y, up) => {
                let Some(l) = self.layout() else { return };
                if render::in_rect(l.drawers, x, y) {
                    self.act(if up { Act::PrevSlot } else { Act::NextSlot });
                } else if let Some(p) = render::tile_at(&l, &self.board, x, y) {
                    self.cursor = p;
                    self.rotate_at(p);
                }
            }
        }
    }

    /// What the launch page offers, in order.
    pub fn menu(&self) -> Vec<(MenuItem, String)> {
        let start = if self.returning || !self.undo.is_empty() || self.solved.iter().any(|&s| s) { "Continue" } else { "Start" };
        let sound = if self.audio.muted { "Sound: off" } else { "Sound: on" };
        vec![(MenuItem::Continue, start.into()), (MenuItem::Notebook, "The Master's Notebook".into()), (MenuItem::Sound, sound.into()), (MenuItem::Quit, "Quit".into())]
    }

    fn choose(&mut self, item: MenuItem) {
        match item {
            MenuItem::Continue => {
                self.screen = Screen::Desk;
                self.audio.play(Sfx::Place);
            }
            MenuItem::Notebook => {
                self.screen = Screen::Desk;
                self.open_page(0);
            }
            MenuItem::Sound => {
                self.toggle_sound();
                self.audio.play(Sfx::Select);
            }
            MenuItem::Quit => self.quit = true,
        }
    }

    fn title_act(&mut self, a: Act) {
        let menu = self.menu();
        let rects = render::title_items(self.area, menu.len());
        let at = |x: u16, y: u16| rects.iter().position(|r| render::in_rect(*r, x, y));
        match a {
            Act::Move(Dir::N) | Act::PrevSlot => self.title_sel = (self.title_sel + menu.len() - 1) % menu.len(),
            Act::Move(Dir::S) | Act::NextSlot => self.title_sel = (self.title_sel + 1) % menu.len(),
            Act::Place => self.choose(menu[self.title_sel].0),
            Act::Mute => self.toggle_sound(),
            Act::Hover(x, y) => {
                if let Some(i) = at(x, y) {
                    self.title_sel = i;
                }
            }
            Act::Down(x, y, false) => {
                if let Some(i) = at(x, y) {
                    self.title_sel = i;
                    self.choose(menu[i].0);
                }
            }
            _ => {}
        }
    }

    fn book_act(&mut self, a: Act) {
        let Overlay::Book { page, scroll } = self.overlay else { return };
        let Some(l) = self.layout() else { return };
        let book = render::book_layout(&l);
        match a {
            Act::Back | Act::Notebook => {
                self.overlay = Overlay::None;
                self.example = None;
                self.book_point = None;
            }
            Act::Hover(x, y) | Act::Drag(x, y, _) => self.book_point = self.example_tile(x, y),
            Act::Move(Dir::N) | Act::PrevSlot => {
                if let Some(i) = (0..page).rev().find(|&i| self.page_unlocked(i)) {
                    self.open_page(i);
                }
            }
            Act::Move(Dir::S) | Act::NextSlot => {
                if let Some(i) = (page + 1..self.pages.len()).find(|&i| self.page_unlocked(i)) {
                    self.open_page(i);
                }
            }
            Act::Scroll(x, y, up) => {
                if render::in_rect(book.list, x, y) {
                    self.book_act(if up { Act::Move(Dir::N) } else { Act::Move(Dir::S) });
                } else {
                    let s = if up { scroll.saturating_sub(1) } else { scroll + 1 };
                    self.overlay = Overlay::Book { page, scroll: s };
                }
            }
            Act::Down(x, y, _) => {
                if render::in_rect(book.list, x, y) {
                    let i = (y - book.list.y) as usize;
                    if i < self.pages.len() {
                        self.open_page(i);
                    }
                } else if let Some(p) = self.example_tile(x, y) {
                    // Flip a switch or press a button on the page's example circuit.
                    self.book_point = Some(p);
                    let Some((_, b, live)) = &mut self.example else { return };
                    if let Cell::Part(i) = b.cell(p) {
                        let flipped = match b.parts[i].kind {
                            Kind::Switch { closed } => Some(Kind::Switch { closed: !closed }),
                            Kind::Button { held } => Some(Kind::Button { held: !held }),
                            _ => None,
                        };
                        if let Some(k) = flipped {
                            b.parts[i].kind = k;
                            live.set_switch(i, matches!(k, Kind::Switch { closed: true } | Kind::Button { held: true }));
                            self.audio.play(Sfx::Click);
                        }
                    }
                } else if !render::in_rect(l.overlay, x, y) {
                    self.overlay = Overlay::None;
                    self.example = None;
                    self.book_point = None;
                }
            }
            _ => {}
        }
    }

    /// The Notebook example's tile at a screen position, if the example is drawn there.
    fn example_tile(&self, x: u16, y: u16) -> Option<Pos> {
        let l = self.layout()?;
        let (_, b, _) = self.example.as_ref()?;
        let (ox, oy) = render::book_example_origin(&render::book_layout(&l), b)?;
        if x < ox || y < oy {
            return None;
        }
        let p = Pos { x: ((x - ox) / render::TILE_W) as usize, y: ((y - oy) / render::TILE_H) as usize };
        (p.x < b.w && p.y < b.h).then_some(p)
    }

    /// What the meter and card read: the Notebook example's tile under the pointer while the
    /// Notebook is open, else the cursor on the desk's board. `true` for the example.
    fn metered(&self) -> (&Board, &Live, Pos, bool) {
        if matches!(self.overlay, Overlay::Book { .. })
            && let (Some(p), Some((_, b, live))) = (self.book_point, &self.example)
        {
            return (b, live, p, true);
        }
        (&self.board, &self.live, self.cursor, false)
    }

    /// What the scope watches: as the meter, but a pinned probe wins on the desk's board.
    fn scoped(&self) -> (&Board, &Live, Pos, bool) {
        match self.metered() {
            (_, _, _, false) => (&self.board, &self.live, self.probe.unwrap_or(self.cursor), false),
            on_example => on_example,
        }
    }

    fn drawer_act(&mut self, a: Act) {
        let Some(l) = self.layout() else { return };
        let (across, down) = render::overlay_grid(&l);
        let n = self.slots();
        let Overlay::Drawers { sel, scroll } = &mut self.overlay else { return };
        let mut close = false;
        match a {
            Act::Move(Dir::E) | Act::NextSlot => *sel = (*sel + 1).min(n - 1),
            Act::Move(Dir::W) | Act::PrevSlot => *sel = sel.saturating_sub(1),
            Act::Move(Dir::S) => *sel = (*sel + across).min(n - 1),
            Act::Move(Dir::N) => *sel = sel.saturating_sub(across),
            Act::Slot(k) if k < n => {
                *sel = k;
                close = true;
            }
            Act::Place => close = true,
            Act::Back | Act::Drawer => {
                self.overlay = Overlay::None;
                return;
            }
            Act::Scroll(_, _, up) => {
                let rows = n.div_ceil(across);
                *scroll = if up { scroll.saturating_sub(1) } else { (*scroll + 1).min(rows.saturating_sub(down)) };
                return;
            }
            Act::Down(x, y, _) => match render::overlay_card_at(&l, n, *scroll, x, y) {
                Some(k) => {
                    *sel = k;
                    close = true;
                }
                None => {
                    self.overlay = Overlay::None;
                    return;
                }
            },
            _ => return,
        }
        // Keep the selection in view.
        let row = *sel / across;
        if row < *scroll {
            *scroll = row;
        } else if row >= *scroll + down {
            *scroll = row + 1 - down;
        }
        if close {
            let k = *sel;
            self.overlay = Overlay::None;
            self.pick_slot(k);
        }
    }

    fn mouse_down(&mut self, x: u16, y: u16, right: bool) {
        let Some(l) = self.layout() else { return };
        if let Some(a) = render::button_at(&l, x, y) {
            if a == Act::Probe && self.probe.is_none() {
                self.probe_armed = true;
                self.say("Click a part or copper to clip the scope's probe on.");
                self.audio.play(Sfx::Select);
            } else {
                self.act(a);
            }
            return;
        }
        if let Some(e) = render::edge_at(&l, x, y) {
            if right {
                // Right-click on an edge puts it back where it was.
                match e {
                    Edge::Left => self.splits.left = None,
                    Edge::Right => self.splits.right = None,
                    Edge::Bottom => self.splits.bottom = None,
                }
                log::info!("edge {e:?} reset");
            } else {
                self.resizing = Some(e);
            }
            return;
        }
        if render::in_rect(render::notebook_button(&l), x, y) {
            self.act(Act::Notebook);
            return;
        }
        if let Some(s) = render::slot_at(&l, x, y) {
            self.pick_slot(s);
            return;
        }
        let arrived = (0..self.jobs.len()).filter(|&i| self.unlocked(i)).count();
        if render::in_rect(l.inbox, x, y) && y > l.inbox.y {
            let row = (y - l.inbox.y - 1) as usize;
            if render::inbox_compact(&l, arrived) {
                // The second line steps back (left half) or on (right half).
                if row == 1 {
                    let left = x < l.inbox.x + l.inbox.width / 2;
                    self.act(if left { Act::PrevJob } else { Act::NextJob });
                    return;
                }
            } else if row < arrived + usize::from(arrived < self.jobs.len()) {
                self.open_job(row);
                return;
            }
        }
        if y == l.drawers.y && render::in_rect(l.drawers, x, y) {
            self.act(Act::Drawer);
            return;
        }
        let Some(p) = render::tile_at(&l, &self.board, x, y) else { return };
        self.cursor = p;
        if self.probe_armed {
            self.probe_armed = false;
            self.pin_probe(p);
            return;
        }
        let before = self.snapshot();
        let right = right || self.pick == Pick::Eraser;
        if right {
            self.erase(p);
        } else if let Cell::Part(i) = self.board.cell(p)
            && matches!(self.board.parts[i].kind, Kind::Button { .. })
        {
            self.hold(i, None);
        }
        self.stroke = Some(Stroke { last: p, moved: false, right, before });
    }

    fn mouse_drag(&mut self, x: u16, y: u16, right: bool) {
        let right = right || self.pick == Pick::Eraser;
        if let Some(e) = self.resizing {
            let a = self.area;
            match e {
                Edge::Left => self.splits.left = Some(x.saturating_sub(a.x)),
                Edge::Right => self.splits.right = Some((a.x + a.width).saturating_sub(x)),
                Edge::Bottom => self.splits.bottom = Some((a.y + a.height).saturating_sub(1 + y)),
            }
            return;
        }
        let Some(l) = self.layout() else { return };
        let Some(p) = render::tile_at(&l, &self.board, x, y) else { return };
        self.cursor = p;
        let Some(mut last) = self.stroke.as_ref().map(|s| s.last) else { return };
        if p == last {
            return;
        }
        // Walk tile by tile toward the pointer, laying (or erasing) as we go.
        while last != p {
            let d = if last.x < p.x {
                Dir::E
            } else if last.x > p.x {
                Dir::W
            } else if last.y < p.y {
                Dir::S
            } else {
                Dir::N
            };
            let next = self.board.neighbour(last, d).unwrap();
            if right {
                self.erase(next);
            } else {
                self.lay(last, d);
            }
            last = next;
        }
        if let Some(s) = &mut self.stroke {
            s.last = p;
            s.moved = true;
            let _ = right;
        }
    }

    fn mouse_up(&mut self, x: u16, y: u16) {
        if let Some((_, _, _, moved)) = self.pan.take() {
            // A middle click that didn't drag clips the probe on.
            if !moved
                && let Some(l) = self.layout()
                && let Some(p) = render::tile_at(&l, &self.board, x, y)
            {
                self.pin_probe(p);
            }
            return;
        }
        if let Some(e) = self.resizing.take() {
            log::info!("edge {e:?} dragged: {:?}", self.splits);
            return;
        }
        if matches!(self.held, Some((_, None))) {
            self.release();
        }
        let Some(s) = self.stroke.take() else { return };
        if !s.moved && !s.right {
            // A click: place, flip, or toggle a junction.
            match self.board.cell(s.last) {
                Cell::Empty => self.place(s.last),
                Cell::Trace(m) if m != 0b1111 && matches!(self.pick, Pick::Item(_)) => self.place(s.last),
                Cell::Part(i) if matches!(self.board.parts[i].kind, Kind::Switch { .. }) => self.operate(s.last),
                Cell::Trace(0b1111) | Cell::Jumper => self.operate(s.last),
                _ => {}
            }
            return;
        }
        // A stroke is one step of undo.
        if !s.right && self.board != s.before.0 {
            self.undo.push(s.before);
            self.redo.clear();
        }
    }

    // ------------------------------------------------------------ time

    pub fn update(&mut self, dt: f64) {
        self.time += dt;
        self.autosave();
        if let Some((_, Some(until))) = self.held
            && self.time >= until
        {
            self.release();
        }
        if self.message.as_ref().is_some_and(|(_, until)| self.time >= *until) {
            self.message = None;
        }
        self.acc += dt.min(0.1);
        while self.acc >= STEP {
            self.acc -= STEP;
            if self.unsolvable {
                continue;
            }
            if self.live.sim.step(STEP).is_err() {
                self.unsolvable = true;
                self.say("The circuit can't be solved: two sources are fighting.");
            }
        }
        for i in 0..self.board.parts.len() {
            if self.live.reading(&self.board, i).burnt && !self.board.parts[i].burnt {
                self.board.parts[i].burnt = true;
                let label = self.board.parts[i].label.clone();
                self.say(format!("{label} burnt out! Press f or click MEND to replace it."));
                self.audio.play(Sfx::Pop);
            }
        }
        self.flow = self.live.copper_currents(&self.board);
        if let Some((_, _, live)) = &mut self.example {
            for _ in 0..(dt.min(0.1) / STEP) as usize {
                let _ = live.sim.step(STEP);
            }
        }
        self.audio.set_current(self.supply_amps());
        let (b, live, target, on_example) = self.scoped();
        // An empty hole has nothing to watch: the scope stays blank rather than drawing zero.
        let (v, _) = reading_on(b, live, target);
        let empty = matches!(b.cell(target), Cell::Empty | Cell::Blocked);
        if self.scope_target != Some((on_example, target)) {
            self.scope_target = Some((on_example, target));
            self.scope.clear();
        }
        if empty {
            self.scope.clear();
            return;
        }
        self.scope.push_back(v);
        while self.scope.len() > SCOPE_LEN {
            self.scope.pop_front();
        }
    }

    pub fn snapshot_log(&self) {
        let overlay = match self.overlay {
            Overlay::None => "none",
            Overlay::Drawers { .. } => "drawers",
            Overlay::Report(_) => "report",
            Overlay::Book { .. } => "book",
        };
        log::debug!(
            "snap t={:.1} probe={:?} cursor={},{} pick={:?} facing={:?} copper={} parts={} amps={:.4} burnt={} overlay={overlay} done={} job={}",
            self.time,
            self.probe,
            self.cursor.x,
            self.cursor.y,
            self.pick,
            self.facing,
            self.copper_mode,
            self.board.parts.len(),
            self.supply_amps(),
            self.board.parts.iter().filter(|p| p.burnt).count(),
            self.done(),
            self.job.id,
        );
    }

    // ------------------------------------------------------------ drawing

    fn card(&self) -> Option<Card> {
        let (board, live, at, on_example) = self.metered();
        let (i, kind, label) = match board.cell(at) {
            Cell::Part(i) => (Some(i), board.parts[i].kind, board.parts[i].label.clone()),
            Cell::Empty if !on_example => match self.pick {
                Pick::Item(k) => (None, self.drawer[k].kind, "in the drawer".into()),
                Pick::Copper | Pick::Eraser => return None,
            },
            _ => return None,
        };
        let price = match i {
            _ if on_example => "in the Notebook".to_string(),
            Some(i) if board.parts[i].fixed => "fixed".to_string(),
            _ => self.job.drawer.iter().find(|it| same_part(it.kind, kind)).map(|it| format!("{:.2} kr", it.price)).unwrap_or_default(),
        };
        let now = match i {
            Some(i) => {
                let r = live.reading(board, i);
                if r.burnt { "burnt out".to_string() } else { format!("{}  {}", si(r.volts, "V"), si(r.amps, "A")) }
            }
            None => String::new(),
        };
        let (side, rule): ([String; 3], (&str, &str)) = match kind {
            Kind::Resistor { ohms, watts } => ([si(ohms, "Ω"), format!("{watts} W"), render::band_names(ohms)], ("Ohm's law", "I = V / R")),
            Kind::Lamp { ohms, watts } => ([si(ohms, "Ω") + " hot", format!("{watts} W"), String::new()], ("Glows", "with power")),
            Kind::Battery { volts, r_int } => {
                let what = match volts {
                    v if (v - 4.5).abs() < 0.01 => "3 AA cells",
                    v if (v - 9.0).abs() < 0.01 => "a 9 V block",
                    v if (v - 12.0).abs() < 0.01 => "a van battery",
                    _ => "",
                };
                ([si(volts, "V"), format!("{} inside", si(r_int, "Ω")), what.into()], ("Source", "+ pushes out"))
            }
            Kind::Supply { volts, limit } => ([si(volts, "V"), format!("limit {}", si(limit, "A")), String::new()], ("Limits", "its current")),
            Kind::Switch { closed } => (["slide switch".into(), if closed { "on".into() } else { "off".into() }, String::new()], ("Off", "breaks the loop")),
            Kind::Button { held } => (["push button".into(), if held { "held".into() } else { "up".into() }, String::new()], ("Momentary", "while held")),
            Kind::Led(c) => ([c.name().into(), "about 2 V".into(), "30 mA max".into()], ("Polarised", "long leg to +")),
            Kind::Capacitor { farads } => ([si(farads, "F"), String::new(), String::new()], ("Stores", "charge")),
            Kind::TestPoint => (["test point".into(), String::new(), String::new()], ("Measures", "volts here")),
        };
        let noun = match kind {
            Kind::Resistor { .. } => "Resistor",
            Kind::Lamp { .. } => "Lamp",
            Kind::Battery { .. } => "Battery",
            Kind::Supply { .. } => "Supply",
            Kind::Switch { .. } => "Switch",
            Kind::Button { .. } => "Button",
            Kind::Led(_) => "LED",
            Kind::Capacitor { .. } => "Capacitor",
            Kind::TestPoint => "Test point",
        };
        Some(Card { name: format!("{noun} {label}"), kind, price, side, now, rule: (rule.0.into(), rule.1.into()) })
    }

    fn slot_views(&self) -> Vec<Slot> {
        let mut v = vec![Slot { name: "Copper trace".into(), count: "free".into(), price: String::new(), kind: None }];
        for it in &self.drawer {
            let count = match it.count {
                Some(n) => format!("x{n}"),
                None => "any".into(),
            };
            v.push(Slot { name: part_name(it.kind), count, price: format!("{:.2} kr", it.price), kind: Some(it.kind) });
        }
        v
    }

    pub fn draw(&self, buf: &mut Buffer) {
        let area = self.area;
        if self.screen == Screen::Title {
            let solved = self.solved.iter().filter(|&&s| s).count();
            let line = format!("{solved} of {} jobs done · ledger {:.2} kr", self.jobs.len(), self.ledger);
            render::title(buf, area, &self.menu().into_iter().map(|m| m.1).collect::<Vec<_>>(), self.title_sel, &line, self.time);
            return;
        }
        let Some(l) = self.layout() else {
            render::too_small(buf, area);
            return;
        };
        // The part in hand, faded where it would go: on an empty hole as it faces, on straight
        // copper turned to lie along it, nowhere else.
        let ghost = match (self.pick, self.overlay_is_none()) {
            (Pick::Item(i), true) => {
                let kind = self.drawer[i].kind;
                match self.board.cell(self.cursor) {
                    Cell::Empty => Some((kind, self.facing)),
                    Cell::Trace(m) => in_line(m, kind, self.facing).map(|f| (kind, f)),
                    _ => None,
                }
            }
            _ => None,
        };
        let view = BoardView {
            board: &self.board,
            live: Some(&self.live),
            flow: Some(&self.flow),
            cursor: Some(self.cursor),
            ghost,
            probe: self.probe,
            eraser: self.pick == Pick::Eraser,
            focus: Some((self.job_at, self.job.board.w, self.job.board.h)),
            time: self.time,
        };
        let spec = self
            .job
            .cases
            .iter()
            .map(|c| (c.name.clone(), None))
            .collect();
        let (_, meter) = {
            let (b, live, at, _) = self.metered();
            reading_on(b, live, at)
        };
        let amps = self.supply_amps();
        let status = if self.unsolvable {
            "can't solve".to_string()
        } else if amps > 1e-6 {
            format!("{} flowing", si(amps, "A"))
        } else {
            "no current".to_string()
        };
        let (scoped, _, target, on_example) = self.scoped();
        let what = match scoped.cell(target) {
            Cell::Part(i) => format!("{} current", scoped.parts[i].label),
            Cell::Trace(_) | Cell::Jumper => "copper voltage".into(),
            _ => "nothing".into(),
        };
        let scope_unit = match scoped.cell(target) {
            Cell::Trace(_) | Cell::Jumper => "V",
            _ => "A",
        };
        let scope_label = if on_example {
            format!("Notebook: {what}")
        } else if self.probe.is_some() {
            format!("probe: {what}")
        } else {
            format!("cursor: {what} · p pins a probe")
        };
        let edge = self.resizing.or(self.hover_edge);
        let hints = if edge.is_some() {
            "drag this edge to resize the panels · right-click it to put it back".to_string()
        } else if self.pick == Pick::Eraser {
            "eraser: click or drag to remove · space erases at the cursor · Esc or ERASE puts it down".to_string()
        } else if self.copper_mode {
            "arrows lay copper · c stops laying · x erases · Esc stops".to_string()
        } else {
            "arrows move · shift+arrows copper · space place · r rotate · x erase · e flip · t test · d drawers".to_string()
        };
        let d = DeskView {
            title: format!("{} · {}", self.job.name, self.job.customer),
            ledger: format!("{:.2} kr", self.ledger),
            inbox: self
                .jobs
                .iter()
                .enumerate()
                .filter(|&(i, _)| self.unlocked(i))
                .map(|(i, j)| (j.name.clone(), i == self.current, self.marks(i)))
                .collect(),
            sealed: (0..self.jobs.len()).filter(|&i| !self.unlocked(i)).count(),
            customer: self.job.customer.clone(),
            note: {
                let mut note = self.job.note.clone();
                let titles: Vec<String> = self.job.book.iter().filter_map(|id| self.pages.iter().find(|p| &p.id == id)).map(|p| p.title.clone()).collect();
                if !titles.is_empty() {
                    note.push(format!("Notebook: {}.", titles.join(", ")));
                }
                note
            },
            spec,
            goals: self.goals(self.current, None).into_iter().skip(1).collect(),
            works: self.marks(self.current)[0],
            slots: self.slot_views(),
            pick: self.slot_index(),
            copper_mode: self.copper_mode,
            meter,
            scope: self.scope.iter().copied().collect(),
            scope_label,
            scope_unit,
            card: self.card(),
            status,
            hints,
            message: self.message.as_ref().map(|(m, _)| m.clone()),
            edge,
        };
        render::desk(buf, area, &l, &view, &d);
        match &self.overlay {
            Overlay::None => {}
            Overlay::Drawers { sel, scroll } => render::drawer_overlay(buf, &l, &self.slot_views(), *sel, *scroll),
            Overlay::Book { page, scroll } => {
                let titles: Vec<(String, bool)> = self.pages.iter().enumerate().map(|(i, p)| (p.title.clone(), self.page_unlocked(i))).collect();
                let example = self.example.as_ref().filter(|(i, _, _)| i == page).map(|(_, b, live)| {
                    let flow = live.copper_currents(b);
                    let readings = (0..b.parts.len())
                        .filter(|&i| b.parts[i].kind.pins() == 2 && !b.parts[i].kind.is_source())
                        .map(|i| {
                            let r = live.reading(b, i);
                            format!("{} {} {}", b.parts[i].label, si(r.volts, "V"), si(r.amps, "A"))
                        })
                        .collect::<Vec<_>>();
                    (b, live, flow, readings)
                });
                render::book(buf, &l, &titles, *page, &self.pages[*page], *scroll, example.as_ref().map(|(b, live, flow, r)| (*b, *live, flow.as_slice(), r.as_slice())), self.time);
            }
            Overlay::Report(r) => render::report(buf, &l, r),
        }
    }

    fn overlay_is_none(&self) -> bool {
        matches!(self.overlay, Overlay::None)
    }
}

/// Builds the bike lamp by keyboard, as a player would: 220 ohm along the top, the switch along
/// the bottom. For tests and the scripted shots.
pub fn build_bike_lamp(a: &mut App) {
    let go = |a: &mut App, x: usize, y: usize| a.cursor = a.at(x, y);
    // Resistor 220 is drawer slot 3, the switch slot 5.
    a.act(Act::Slot(3));
    go(a, 4, 1);
    a.act(Act::Place);
    a.act(Act::Slot(5));
    go(a, 4, 3);
    a.act(Act::Place);
    // Top: from the battery up, along, down into the LED.
    go(a, 1, 2);
    a.act(Act::Lay(Dir::N));
    for _ in 0..7 {
        a.act(Act::Lay(Dir::E));
    }
    a.act(Act::Lay(Dir::S));
    // Bottom: from the battery down, along, up into the LED.
    go(a, 1, 2);
    a.act(Act::Lay(Dir::S));
    for _ in 0..7 {
        a.act(Act::Lay(Dir::E));
    }
    a.act(Act::Lay(Dir::N));
}

/// What the scope and meter read at a tile of a running board: (the scope's value, the meter).
fn reading_on(b: &Board, live: &Live, at: Pos) -> (f64, Meter) {
    match b.cell(at) {
        Cell::Part(i) => {
            let r = live.reading(b, i);
            let label = &b.parts[i].label;
            let what = if b.parts[i].kind.pins() == 1 { format!("{label} to ground") } else { format!("across {label}") };
            let second = if b.parts[i].kind.pins() == 1 { String::new() } else { format!("{} through", si(r.amps, "A")) };
            (r.amps, Meter { value: r.volts, unit: "V", what, second })
        }
        Cell::Trace(m) => {
            let d = Dir::ALL.into_iter().find(|d| m & d.bit() != 0).unwrap_or(Dir::N);
            let v = live.net.net_at(b, at, d).map(|n| live.net_volts(n)).unwrap_or(0.0);
            (v, Meter { value: v, unit: "V", what: "copper to ground".into(), second: String::new() })
        }
        Cell::Jumper => {
            let v = live.net.net_at(b, at, Dir::E).map(|n| live.net_volts(n)).unwrap_or(0.0);
            (v, Meter { value: v, unit: "V", what: "jumper, east-west".into(), second: String::new() })
        }
        _ => (0.0, Meter { value: 0.0, unit: "V", what: "nothing here".into(), second: String::new() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let jobs = (0..crate::job::JOBS.len()).map(Job::load).collect();
        let mut a = App::new(jobs, Progress::default(), Audio::new(true));
        a.area = Rect::new(0, 0, 100, 30);
        a.screen = Screen::Desk;
        a
    }

    use super::build_bike_lamp;

    #[test]
    fn building_the_bike_lamp_by_keyboard_passes_and_pays() {
        let mut a = app();
        build_bike_lamp(&mut a);
        a.update(0.2);
        let led = a.board.part("LED1").unwrap();
        assert!(a.live.reading(&a.board, led).amps > 0.010);
        a.act(Act::Test);
        let Overlay::Report(r) = &a.overlay else { panic!("no report") };
        assert!(r.pass, "{:?}", r.rows);
        // The fee less the parts, and a 4 kr tip for matching the master's battery life.
        assert!((a.ledger - (40.0 - 2.10 + 4.0)).abs() < 1e-9, "{}", a.ledger);
        assert_eq!(a.drawer[2].count, Some(1));
        assert_eq!(a.marks(0), [true, true, true]);
        // Testing again pays nothing more.
        a.act(Act::Back);
        a.act(Act::Test);
        assert!((a.ledger - 41.9).abs() < 1e-9, "{}", a.ledger);
    }

    #[test]
    fn a_build_that_drains_the_battery_misses_its_tick_until_it_is_better() {
        let mut a = app();
        build_bike_lamp(&mut a);
        let r = a.board.part("R1").unwrap();
        a.board.parts[r].kind = Kind::Resistor { ohms: 150.0, watts: 0.25 };
        a.act(Act::Test);
        let Overlay::Report(rep) = &a.overlay else { panic!("no report") };
        assert!(rep.pass);
        assert_eq!(rep.goals.iter().map(|g| g.1).collect::<Vec<_>>(), [true, true, false]);
        assert_eq!(a.marks(0), [true, true, false]);
        assert!((a.ledger - 37.9).abs() < 1e-9, "{}", a.ledger);
        // Back to 220 ohm: the battery tick, and the tip, once.
        a.act(Act::Back);
        a.board.parts[r].kind = Kind::Resistor { ohms: 220.0, watts: 0.25 };
        a.act(Act::Test);
        assert_eq!(a.marks(0), [true, true, true]);
        assert!((a.ledger - 41.9).abs() < 1e-9, "{}", a.ledger);
    }

    #[test]
    fn copper_flows_round_the_built_lamp() {
        let mut a = app();
        build_bike_lamp(&mut a);
        a.update(0.1);
        let t = |x: usize, y: usize| {
            let p = a.at(x, y);
            a.flow[p.y * a.board.w + p.x]
        };
        // Along the top the current runs east.
        assert!(t(2, 1)[Dir::E as usize] > 0.010, "{:?}", t(2, 1));
        // Along the bottom it runs west, back to the battery.
        assert!(t(6, 3)[Dir::W as usize] > 0.010, "{:?}", t(6, 3));
    }

    #[test]
    fn a_failed_test_brings_a_hint() {
        let mut a = app();
        a.act(Act::Test);
        let Overlay::Report(r) = &a.overlay else { panic!("no report") };
        assert!(!r.pass);
        assert!(r.hint.is_some());
    }

    #[test]
    fn undo_takes_back_a_part_and_its_drawer_count() {
        let mut a = app();
        a.act(Act::Slot(3));
        a.cursor = a.at(4, 1);
        a.act(Act::Place);
        assert_eq!(a.drawer[2].count, Some(1));
        a.act(Act::Undo);
        assert_eq!(a.drawer[2].count, Some(2));
        assert_eq!(a.board.cell(a.at(4, 1)), Cell::Empty);
        a.act(Act::Redo);
        assert_eq!(a.drawer[2].count, Some(1));
    }

    #[test]
    fn a_pinned_probe_keeps_the_scope_on_its_part() {
        let mut a = app();
        build_bike_lamp(&mut a);
        a.cursor = a.at(8, 2);
        a.act(Act::Probe);
        assert_eq!(a.probe, Some(a.at(8, 2)));
        a.update(0.05);
        a.cursor = a.at(0, 0);
        a.update(0.05);
        // Still the LED's current, not the empty corner's nothing.
        assert!(a.scope.back().copied().unwrap_or(0.0) > 0.010);
        a.act(Act::Probe);
        assert_eq!(a.probe, None);
    }

    #[test]
    fn dragging_an_edge_resizes_the_panels_but_not_the_board() {
        let mut a = app();
        a.area = Rect::new(0, 0, 140, 45);
        let l = a.layout().unwrap();
        let before = (l.board.width,);
        let y = l.board.y + 3;
        a.act(Act::Down(l.board.x, y, false));
        a.act(Act::Drag(l.board.x - 6, y, false));
        a.act(Act::Up(l.board.x - 6, y));
        let l2 = a.layout().unwrap();
        assert_eq!(l2.inbox.width, l.inbox.width - 6);
        assert_eq!(l2.board.width, before.0 + 6);
        // The tiles are the same size; the wider panel shows more of them.
        let (vx, vy) = l2.view;
        assert_eq!(render::tile_at(&l2, &a.board, l2.origin.0 + 4, l2.origin.1 + 2), Some(Pos { x: vx, y: vy }));
        assert_eq!(render::tile_at(&l2, &a.board, l2.origin.0 + 5, l2.origin.1), Some(Pos { x: vx + 1, y: vy }));
        assert!(render::tiles_shown(&l2).0 > render::tiles_shown(&l).0);
        // Right-click puts it back.
        a.act(Act::Down(l2.board.x, y, true));
        assert_eq!(a.splits.left, None);
    }

    #[test]
    fn the_eraser_removes_what_it_is_dragged_over() {
        let mut a = app();
        build_bike_lamp(&mut a);
        let l = a.layout().unwrap();
        let job_at = a.job_at;
        let at = |x: u16, y: u16| {
            let p = Pos { x: job_at.x + x as usize, y: job_at.y + y as usize };
            let (sx, sy) = render::screen_of(&l, p).unwrap();
            (sx + 2, sy + 1)
        };
        a.act(Act::Eraser);
        let (sx, sy) = at(2, 1);
        let (ex, ey) = at(5, 1);
        a.act(Act::Down(sx, sy, false));
        a.act(Act::Drag(ex, ey, false));
        a.act(Act::Up(ex, ey));
        for x in 2..=5 {
            assert_eq!(a.board.cell(a.at(x, 1)), Cell::Empty, "tile {x}");
        }
        // The resistor went back in its drawer.
        assert_eq!(a.drawer[2].count, Some(2));
        a.act(Act::Back);
        assert_eq!(a.pick, Pick::Copper);
    }

    #[test]
    fn a_part_dropped_on_straight_copper_splits_it() {
        let mut a = app();
        // Copper all the way round, no resistor yet.
        a.cursor = a.at(1, 2);
        a.act(Act::Lay(Dir::N));
        for _ in 0..7 {
            a.act(Act::Lay(Dir::E));
        }
        a.act(Act::Lay(Dir::S));
        a.cursor = a.at(1, 2);
        a.act(Act::Lay(Dir::S));
        for _ in 0..7 {
            a.act(Act::Lay(Dir::E));
        }
        a.act(Act::Lay(Dir::N));
        // A 220 ohm on the top run, while facing north: it turns to lie along the copper.
        a.facing = Dir::N;
        a.act(Act::Slot(3));
        a.cursor = a.at(4, 1);
        a.act(Act::Place);
        let r = a.board.part("R1").expect("placed on the copper");
        assert_eq!(a.board.parts[r].facing, Dir::W);
        // On a corner it is refused.
        a.cursor = a.at(1, 1);
        a.act(Act::Place);
        assert!(matches!(a.board.cell(a.at(1, 1)), Cell::Trace(_)));
        a.update(0.05);
        let led = a.board.part("LED1").unwrap();
        assert!(!a.board.parts[led].burnt);
        assert!(a.live.reading(&a.board, led).amps > 0.010);
    }

    #[test]
    fn solving_a_job_unlocks_the_next_and_each_keeps_its_board() {
        let mut a = app();
        assert!(!a.unlocked(1));
        a.open_job(1);
        assert_eq!(a.current, 0);
        build_bike_lamp(&mut a);
        a.act(Act::Test);
        a.act(Act::Back);
        assert!(a.unlocked(1));
        a.act(Act::NextJob);
        assert_eq!((a.current, a.job.id.as_str()), (1, "porch-lamp"));
        assert!(a.board.part("R1").is_none());
        a.act(Act::PrevJob);
        assert_eq!(a.current, 0);
        assert!(a.board.part("R1").is_some(), "the bike lamp came back as it was left");
    }

    #[test]
    fn the_launch_page_starts_the_desk() {
        let mut a = app();
        a.screen = Screen::Title;
        assert_eq!(a.menu()[0].1, "Start");
        a.act(Act::Place);
        assert_eq!(a.screen, Screen::Desk);
        a.act(Act::Back);
        assert_eq!(a.screen, Screen::Title);
    }

    #[test]
    fn the_notebook_opens_unlocked_pages_and_flips_example_switches() {
        let mut a = app();
        a.act(Act::Notebook);
        assert!(matches!(a.overlay, Overlay::Book { page: 0, .. }));
        // The page on switches belongs to the porch lamp, not yet in the inbox.
        let switches = a.pages.iter().position(|p| p.id == "switches").unwrap();
        assert!(!a.page_unlocked(switches));
        for _ in 0..a.pages.len() {
            a.act(Act::Move(Dir::S));
        }
        let Overlay::Book { page, .. } = a.overlay else { panic!() };
        assert!(a.page_unlocked(page));
        a.act(Act::Back);
        assert!(matches!(a.overlay, Overlay::None));
    }

    #[test]
    fn the_meter_and_scope_read_the_notebooks_example() {
        let mut a = app();
        a.area = Rect::new(0, 0, 140, 50);
        a.act(Act::Notebook);
        let i = a.pages.iter().position(|p| p.id == "colour-code").unwrap();
        a.open_page(i);
        let (_, b, _) = a.example.as_ref().unwrap();
        let r1 = b.parts[b.part("R1").unwrap()].pos;
        let (ox, oy) = render::book_example_origin(&render::book_layout(&a.layout().unwrap()), b).unwrap();
        a.act(Act::Hover(ox + r1.x as u16 * render::TILE_W + 2, oy + r1.y as u16 * render::TILE_H + 1));
        assert_eq!(a.book_point, Some(r1));
        for _ in 0..10 {
            a.update(0.016);
        }
        let (b, live, at, on_example) = a.metered();
        assert!(on_example);
        let (amps, meter) = reading_on(b, live, at);
        // 4.5 V across 220 + 4700 ohm (and the battery's 0.45): about 915 uA, 0.2 V across R1.
        assert!((amps - 0.000915).abs() < 0.00001, "{amps}");
        assert!((meter.value - 0.201).abs() < 0.002, "{}", meter.value);
        assert!(a.scope.len() >= 10);
        assert_eq!(a.card().unwrap().name, "Resistor R1");
        // Closing the Notebook hands the meter back to the desk.
        a.act(Act::Back);
        assert!(!a.metered().3);
    }

    #[test]
    fn an_edit_is_saved_a_couple_of_seconds_after_it_settles() {
        let mut a = app();
        a.update(0.016);
        assert_eq!(a.changed_at, None);
        a.cursor = a.at(1, 2);
        a.act(Act::Lay(Dir::N));
        a.update(0.016);
        assert!(a.changed_at.is_some());
        for _ in 0..60 {
            a.update(0.016);
        }
        assert!(a.changed_at.is_some(), "saved too soon");
        for _ in 0..80 {
            a.update(0.016);
        }
        assert_eq!(a.changed_at, None, "never saved");
    }

    #[test]
    fn fixed_parts_stay() {
        let mut a = app();
        a.cursor = a.at(1, 2);
        a.act(Act::Erase);
        assert!(a.board.part("BT1").is_some());
    }

    #[test]
    fn a_led_without_resistor_burns_and_mends() {
        let mut a = app();
        a.cursor = a.at(1, 2);
        a.act(Act::Lay(Dir::N));
        for _ in 0..7 {
            a.act(Act::Lay(Dir::E));
        }
        a.act(Act::Lay(Dir::S));
        a.cursor = a.at(1, 2);
        a.act(Act::Lay(Dir::S));
        for _ in 0..7 {
            a.act(Act::Lay(Dir::E));
        }
        a.act(Act::Lay(Dir::N));
        a.update(0.05);
        let led = a.board.part("LED1").unwrap();
        assert!(a.board.parts[led].burnt);
        a.act(Act::Mend);
        assert!(!a.board.parts[led].burnt);
    }

    #[test]
    fn the_drawer_overlay_picks_and_snaps_back() {
        let mut a = app();
        a.act(Act::Drawer);
        assert!(matches!(a.overlay, Overlay::Drawers { .. }));
        a.act(Act::Move(Dir::E));
        a.act(Act::Move(Dir::E));
        a.act(Act::Place);
        assert!(matches!(a.overlay, Overlay::None));
        assert_eq!(a.pick, Pick::Item(1));
    }

    #[test]
    fn a_mouse_drag_lays_copper_in_one_undo_step() {
        let mut a = app();
        let l = a.layout().unwrap();
        let job_at = a.job_at;
        let at = |x: u16, y: u16| {
            let p = Pos { x: job_at.x + x as usize, y: job_at.y + y as usize };
            let (sx, sy) = render::screen_of(&l, p).unwrap();
            (sx + 2, sy + 1)
        };
        let (sx, sy) = at(1, 1);
        a.act(Act::Down(sx, sy, false));
        let (ex, ey) = at(5, 1);
        a.act(Act::Drag(ex, ey, false));
        a.act(Act::Up(ex, ey));
        assert_eq!(a.board.cell(a.at(3, 1)), Cell::Trace(Dir::E.bit() | Dir::W.bit()));
        a.act(Act::Undo);
        assert_eq!(a.board.cell(a.at(3, 1)), Cell::Empty);
    }
}
