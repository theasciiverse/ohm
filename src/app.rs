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
use crate::circuit::{Board, CELSIUS_STEPS, Cell, Dir, Kind, LUX_STEPS, Live, Pos};
use crate::input::Act;
use crate::job::{Item, Job, SANDBOX, Score, Ticks, same_part};
use crate::book::Page;
use crate::progress::Progress;
use crate::render::{self, BoardView, Card, DeskView, Edge, Grow, Layout, Meter, Report, Slot, Splits};

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
    Sandbox,
    Notebook,
    Help,
    Quit,
}

pub enum Overlay {
    None,
    Drawers { sel: usize, scroll: usize },
    Report(Report),
    /// The Master's Notebook open at a page, its text scrolled down by `scroll` lines.
    Book { page: usize, scroll: usize },
    /// The card binder, open at card `sel` of `BINDER`, scrolled down by `scroll` rows.
    Binder { sel: usize, scroll: usize },
}

/// The binder's cards in order, one per kind of part; each kind here is the one drawn.
const BINDER: [Kind; 16] = [
    Kind::Battery { volts: 4.5, r_int: 0.45 },
    Kind::Supply { volts: 5.0, limit: 0.1 },
    Kind::Led(crate::circuit::Colour::Red),
    Kind::Lamp { ohms: 15.0, watts: 1.35 },
    Kind::Resistor { ohms: 220.0, watts: 0.25 },
    Kind::Capacitor { farads: 100e-6, volts: 25.0 },
    Kind::Switch { closed: false },
    Kind::Button { held: false, nc: false },
    Kind::Fuse { amps: 2.0 },
    Kind::Pot { ohms: 10e3, turn: 0.5 },
    Kind::Ldr { lux: 100.0 },
    Kind::Ntc { celsius: 20.0 },
    Kind::Relay { volts: 12.0, nc: false },
    Kind::Diode,
    Kind::Npn { flip: false },
    Kind::Chip(crate::circuit::Chip::Cd4011),
];

/// The binder card a part belongs on.
fn binder_card(k: Kind) -> Option<usize> {
    BINDER.iter().position(|b| std::mem::discriminant(b) == std::mem::discriminant(&k))
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
    /// The sandbox is on the desk instead of a job (`current` stays the job to go back to), and
    /// its board while it isn't.
    pub sandbox: bool,
    sandbox_board: Option<Board>,
    /// Sound as the player last set it, for the save file (`--mute` alone doesn't stick).
    sound_off: bool,
    /// The gadget on the launch page's shelf under the mouse.
    shelf_hover: Option<usize>,
    /// The shelf's page when it can't show every gadget: as turned, else the open job's.
    shelf_page: Option<usize>,
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
    /// The panel grown along its strip, if any, and the panel title under the mouse.
    pub grown: Option<Grow>,
    hover_title: Option<Grow>,
    /// The help page, open and scrolled down this many lines; it covers the launch page or the desk.
    pub help: Option<usize>,
    pub quit: bool,
    pub audio: Audio,
    acc: f64,
    /// Each part's relay state at the last step, and the moments relays moved, for their sound.
    relays: Vec<bool>,
    flips: std::collections::VecDeque<f64>,
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

/// A value as printed on parts: "2.2 kΩ", "1.5 V", no trailing zeros.
fn value(v: f64, unit: &str) -> String {
    let s = si(v, unit);
    match s.split_once(' ') {
        Some((n, u)) if n.contains('.') => format!("{} {u}", n.trim_end_matches('0').trim_end_matches('.')),
        _ => s,
    }
}

/// Capacitance as capacitors are marked: µF from 1 µF up (1000 µF, never 1 mF), nF below.
fn farads_name(farads: f64) -> String {
    if farads >= 1e-6 { format!("{} µF", (farads * 1e6).round()) } else { format!("{} nF", (farads * 1e9).round()) }
}

pub fn part_name(k: Kind) -> String {
    match k {
        Kind::Resistor { ohms, watts } => {
            if watts == 0.25 { format!("Resistor {}", value(ohms, "Ω")) } else { format!("Resistor {} {watts} W", value(ohms, "Ω")) }
        }
        // Bulbs are sold by voltage (the one their hot resistance is for); the watts are on the card.
        Kind::Lamp { ohms, watts } => format!("Lamp {:.1} V", (ohms * watts).sqrt()),
        Kind::Battery { volts, .. } => format!("Battery {}", value(volts, "V")),
        Kind::Supply { volts, .. } => format!("Bench supply {}", value(volts, "V")),
        Kind::Switch { .. } => "Slide switch".into(),
        Kind::Button { nc: false, .. } => "Push button".into(),
        Kind::Led(c) => format!("LED, {}", c.name()),
        // "Cap", so the rating fits the drawer. Electrolytics with their rating.
        // No-break spaces keep each number with its unit when a card wraps the name.
        Kind::Capacitor { farads, volts } if k.polarised() => format!("Cap {} {}", farads_name(farads), value(volts, "V")).replace(" µF", "\u{a0}µF").replace(" V", "\u{a0}V"),
        Kind::Capacitor { farads, .. } => format!("Cap {}", farads_name(farads)).replace(" nF", "\u{a0}nF"),
        Kind::TestPoint => "Test point".into(),
        Kind::Fuse { amps } => format!("Fuse {}", value(amps, "A")),
        Kind::Ldr { .. } => "Light sensor (LDR)".into(),
        Kind::Ntc { .. } => "Thermistor 10 kΩ".into(),
        Kind::Pot { ohms, .. } => format!("Pot {}", value(ohms, "Ω")),
        Kind::Button { nc: true, .. } => "Stop button".into(),
        Kind::Diode => "Diode 1N4001".into(),
        Kind::Npn { .. } => "Transistor BC547".into(),
        Kind::Relay { volts, nc } => format!("Relay {}{}", value(volts, "V"), if nc { " NC" } else { "" }).replace(" V", "\u{a0}V"),
        Kind::Chip(c) => format!("Chip {}", c.name()),
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
            sandbox: false,
            sandbox_board: progress.boards.iter().find(|b| b.0 == SANDBOX).map(|b| b.1.clone()),
            shelf_hover: None,
            shelf_page: None,
            seen: (Board::new(1, 1), Vec::new()),
            changed_at: None,
            area: Rect::new(0, 0, render::MIN_W, render::MIN_H),
            splits: progress.splits,
            resizing: None,
            view: (cursor.x, cursor.y),
            job_at,
            pan: None,
            hover_edge: None,
            grown: None,
            hover_title: None,
            help: None,
            quit: false,
            audio,
            acc: 0.0,
            relays: Vec::new(),
            flips: std::collections::VecDeque::new(),
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
        self.leave_sandbox();
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
        let kept = self.saved[i].take();
        self.on_desk(kept);
    }

    /// Puts `self.job` on the desk: the board and drawers as kept, or fresh.
    fn on_desk(&mut self, kept: Option<(Board, Vec<Item>)>) {
        let (fresh, job_at) = self.job.board.embed(BENCH.0, BENCH.1);
        (self.board, self.drawer) = kept.unwrap_or((fresh, self.job.drawer.clone()));
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

    /// Puts the open job aside and brings out the sandbox, with every kind of part.
    pub fn enter_sandbox(&mut self) {
        if self.sandbox {
            return;
        }
        self.saved[self.current] = Some(self.snapshot());
        self.sandbox = true;
        self.job = Job::sandbox();
        let kept = self.sandbox_board.take().map(|b| (b, self.job.drawer.clone()));
        self.on_desk(kept);
        self.save();
    }

    /// Puts the sandbox aside and brings back the job it left.
    fn leave_sandbox(&mut self) {
        if !self.sandbox {
            return;
        }
        self.sandbox_board = Some(self.board.clone());
        self.sandbox = false;
        self.saved_into_place(self.current);
    }

    /// Writes the ledger, the solved jobs, every board and the panel edges to the save file.
    pub fn save(&self) {
        let mut boards = Vec::new();
        for (i, job) in self.jobs.iter().enumerate() {
            let state = if i == self.current && !self.sandbox { Some((&self.board, &self.drawer)) } else { self.saved[i].as_ref().map(|(b, d)| (b, d)) };
            if let Some((b, d)) = state {
                boards.push((job.id.clone(), b.clone(), d.iter().map(|it| it.count).collect()));
            }
        }
        let solved = self.jobs.iter().zip(&self.solved).filter(|(_, s)| **s).map(|(j, _)| j.id.clone()).collect();
        let best = self.jobs.iter().zip(&self.best).filter_map(|(j, b)| b.map(|(profit, t)| (j.id.clone(), profit, t))).collect();
        let sandbox = if self.sandbox { Some(&self.board) } else { self.sandbox_board.as_ref() };
        if let Some(b) = sandbox {
            boards.push((SANDBOX.into(), b.clone(), Vec::new()));
        }
        let open = Some(self.job.id.clone());
        Progress { ledger: self.ledger, solved, best, splits: self.splits, open, muted: self.sound_off, boards }.save();
    }

    fn layout(&self) -> Option<Layout> {
        render::layout(self.area, &self.board, self.drawer.len() + 1, self.splits, self.view, self.grown)
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
        let live = Live::build(&self.board);
        let old = std::mem::replace(&mut self.live, live);
        self.live.keep_charge(&old);
        // The new circuit's clock starts again at zero.
        self.flips.clear();
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
            Cell::Empty if matches!(kind, Kind::Chip(_)) => {
                if !self.board.fits(kind, at, self.facing, None) {
                    let (w, h) = crate::circuit::footprint(kind, self.facing);
                    self.audio.play(Sfx::Denied);
                    self.say(format!("A chip needs {w} by {h} empty holes, from here to the right and down. Turn it with r."));
                    return;
                }
                self.facing
            }
            Cell::Trace(_) if matches!(kind, Kind::Chip(_)) => {
                self.audio.play(Sfx::Denied);
                self.say("A chip needs empty holes: put it down first, then lay copper to its legs.");
                return;
            }
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
                let before = self.snapshot();
                let to = rotate(self.board.parts[i].facing);
                if !self.board.turn(i, to) {
                    self.audio.play(Sfx::Denied);
                    self.say("There's no room to turn it here.");
                    return;
                }
                self.undo.push(before);
                self.redo.clear();
                log::info!("rotated {} to {:?}", self.board.parts[i].label, to);
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
                // An LDR steps through the light, a thermistor through the temperatures, a pot
                // a tenth of a turn (round again at the end).
                k @ (Kind::Ldr { .. } | Kind::Ntc { .. } | Kind::Pot { .. }) => self.set_state(i, next_state(k)),
                // A transistor's legs bent the other way: its base to the other side.
                Kind::Npn { flip } => self.set_state(i, Kind::Npn { flip: !flip }),
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

    /// Sets a sensor's light or warmth, or a pot's turn, and runs the circuit on from there.
    fn set_state(&mut self, i: usize, kind: Kind) {
        self.board.parts[i].kind = kind;
        log::info!("{} now {}", self.board.parts[i].label, crate::job::kind_words(kind));
        self.audio.play(Sfx::Click);
        self.rebuilt();
    }

    /// Turns a pot by `by` (a share of its whole turn), stopping at the ends.
    fn turn_pot(&mut self, i: usize, by: f64) {
        let Kind::Pot { ohms, turn } = self.board.parts[i].kind else { return };
        self.set_state(i, Kind::Pot { ohms, turn: (((turn + by) * 10.0).round() / 10.0).clamp(0.0, 1.0) });
    }

    fn hold(&mut self, part: usize, until: Option<f64>) {
        if let Kind::Button { held, nc } = &mut self.board.parts[part].kind {
            *held = true;
            self.live.set_switch(part, !*nc);
        }
        self.held = Some((part, until));
        self.audio.play(Sfx::Click);
    }

    fn release(&mut self) {
        if let Some((part, _)) = self.held.take()
            && part < self.board.parts.len()
        {
            if let Kind::Button { held, nc } = &mut self.board.parts[part].kind {
                *held = false;
                self.live.set_switch(part, *nc);
            }
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
        if self.sandbox {
            self.say("The sandbox has no customer, so there's nothing to test. Measure it instead.");
            self.audio.play(Sfx::Denied);
            return;
        }
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
        // A passing build that misses a tick gets the master's word on how he did it.
        let mut notes = Vec::new();
        if pass && !ticks.thrifty {
            notes.push(self.job.hint_parts.clone().unwrap_or_else(|| "The master used fewer parts: is every one of yours needed?".into()));
        }
        if pass && !ticks.frugal && let Some(h) = &self.job.hint_battery {
            notes.push(h.clone());
        }
        log::info!("test bench: {}", if pass { "pass" } else { "fail" });
        self.save();
        self.overlay = Overlay::Report(Report { pass, rows, goals, notes, summary, hint });
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
        if matches!(a, Act::Help) {
            self.help = if self.help.is_some() { None } else { Some(0) };
            log::info!("help {}", if self.help.is_some() { "open" } else { "closed" });
            self.audio.play(Sfx::Select);
            return;
        }
        if let Some(scroll) = self.help {
            self.help_act(a, scroll);
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
            Overlay::Binder { .. } => {
                self.binder_act(a);
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
            Act::Grow(g) => {
                self.grown = if self.grown == Some(g) { None } else { Some(g) };
                log::info!("grown {:?}", self.grown);
                self.audio.play(Sfx::Select);
            }
            Act::Back => {
                // Esc puts down whatever is in hand; with nothing in hand it goes to the launch page.
                if self.copper_mode || self.probe_armed || self.pick == Pick::Eraser {
                    self.copper_mode = false;
                    self.probe_armed = false;
                    if self.pick == Pick::Eraser {
                        self.pick = Pick::Copper;
                    }
                } else if let Some(g) = self.grown {
                    self.act(Act::Grow(g));
                } else {
                    self.screen = Screen::Title;
                    self.save();
                }
            }
            Act::NextJob | Act::PrevJob if self.sandbox => self.say("The sandbox has no jobs. Esc goes back to the launch page."),
            Act::NextJob => self.open_job(self.current + 1),
            Act::PrevJob => {
                if self.current > 0 {
                    self.open_job(self.current - 1);
                }
            }
            Act::Binder => {
                // Open at the part under the cursor, or the one in hand.
                let at = match self.board.cell(self.cursor) {
                    Cell::Part(i) => Some(self.board.parts[i].kind),
                    _ => match self.pick {
                        Pick::Item(i) => Some(self.drawer[i].kind),
                        _ => None,
                    },
                };
                let sel = at.and_then(binder_card).unwrap_or(0);
                self.overlay = Overlay::Binder { sel, scroll: 0 };
                log::info!("binder at {:?}", BINDER[sel]);
                self.audio.play(Sfx::Select);
            }
            Act::Quit | Act::Help => {}
            Act::Hover(x, y) => {
                if let Some(l) = self.layout() {
                    self.hover_edge = render::edge_at(&l, x, y);
                    self.hover_title = render::title_at(&l, x, y);
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
                    // The wheel turns a pot's knob; anything else it rotates.
                    match self.board.cell(p) {
                        Cell::Part(i) if matches!(self.board.parts[i].kind, Kind::Pot { .. }) => self.turn_pot(i, if up { 0.1 } else { -0.1 }),
                        _ => self.rotate_at(p),
                    }
                }
            }
        }
    }

    /// What the launch page offers, in order.
    pub fn menu(&self) -> Vec<(MenuItem, String)> {
        let start = if self.returning || !self.undo.is_empty() || self.solved.iter().any(|&s| s) { "Continue" } else { "Start" };
        vec![(MenuItem::Continue, start.into()), (MenuItem::Sandbox, "Sandbox".into()), (MenuItem::Notebook, "The Master's Notebook".into()), (MenuItem::Help, "How to play".into()), (MenuItem::Quit, "Quit".into())]
    }

    fn choose(&mut self, item: MenuItem) {
        match item {
            MenuItem::Continue => {
                self.leave_sandbox();
                self.screen = Screen::Desk;
                self.audio.play(Sfx::Place);
            }
            MenuItem::Notebook => {
                self.screen = Screen::Desk;
                self.open_page(0);
            }
            MenuItem::Help => self.act(Act::Help),
            MenuItem::Sandbox => {
                self.enter_sandbox();
                self.screen = Screen::Desk;
                self.audio.play(Sfx::Place);
            }
            MenuItem::Quit => self.quit = true,
        }
    }

    fn shelf_page(&self) -> usize {
        let size = render::shelf_page_size(self.area, self.jobs.len());
        let pages = self.jobs.len().div_ceil(size);
        self.shelf_page.unwrap_or(self.current / size).min(pages - 1)
    }

    fn shelf_items(&self) -> Vec<render::ShelfItem> {
        (0..self.jobs.len())
            .map(|i| render::ShelfItem { id: self.jobs[i].id.clone(), arrived: self.unlocked(i), solved: self.solved[i], ticks: self.marks(i) })
            .collect()
    }

    fn help_act(&mut self, a: Act, scroll: usize) {
        let page = render::help_rect(self.area).height.saturating_sub(4) as usize;
        let down = |n: usize| {
            let mut s = scroll;
            for _ in 0..n {
                if !render::help_more(self.area, s) {
                    break;
                }
                s += 1;
            }
            s
        };
        let to = match a {
            Act::Move(Dir::S) | Act::Scroll(_, _, false) => down(if matches!(a, Act::Scroll(..)) { 3 } else { 1 }),
            Act::Move(Dir::N) => scroll.saturating_sub(1),
            Act::Scroll(_, _, true) => scroll.saturating_sub(3),
            Act::NextJob => down(page),
            Act::PrevJob => scroll.saturating_sub(page),
            Act::Back | Act::Place | Act::Down(..) => {
                self.help = None;
                return;
            }
            _ => scroll,
        };
        self.help = Some(to);
    }

    fn title_act(&mut self, a: Act) {
        let menu = self.menu();
        let rects = render::title_items(self.area, menu.len());
        let at = |x: u16, y: u16| rects.iter().position(|r| render::in_rect(*r, x, y));
        let page = self.shelf_page();
        let shelf = render::shelf_slots(self.area, self.jobs.len(), page);
        let on_shelf = |x: u16, y: u16| shelf.iter().position(|r| render::in_rect(*r, x, y));
        // The plank's arrows, or left and right, turn the shelf's pages.
        let pages = self.jobs.len().div_ceil(render::shelf_page_size(self.area, self.jobs.len()));
        let arrows = render::shelf_arrows(self.area, self.jobs.len(), page);
        let turn = match a {
            Act::Move(Dir::W) => Some(false),
            Act::Move(Dir::E) => Some(true),
            Act::Down(x, y, false) => arrows.iter().position(|r| r.is_some_and(|r| render::in_rect(r, x, y))).map(|k| k == 1),
            _ => None,
        };
        if let Some(next) = turn {
            let to = if next { (page + 1).min(pages - 1) } else { page.saturating_sub(1) };
            if to != page {
                self.shelf_page = Some(to);
                self.audio.play(Sfx::Select);
            }
            return;
        }
        if let Act::Hover(x, y) = a {
            self.shelf_hover = on_shelf(x, y);
        }
        // Sound sits in the corner, opposite the ledger.
        if let Act::Down(x, y, false) = a
            && render::in_rect(render::sound_button(self.area), x, y)
        {
            self.toggle_sound();
            self.audio.play(Sfx::Select);
            return;
        }
        // A gadget on the shelf opens its job on the desk.
        if let Act::Down(x, y, false) = a
            && let Some(i) = on_shelf(x, y)
        {
            if self.unlocked(i) {
                self.open_job(i);
                self.screen = Screen::Desk;
            } else {
                self.audio.play(Sfx::Denied);
            }
            return;
        }
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

    /// Whether a binder card is collected: a job that has arrived uses that kind of part, or it
    /// is on the sandbox board.
    fn collected(&self, card: usize) -> bool {
        self.met_in(card).is_some()
    }

    /// Where a binder card's part was first met: the first arrived job with it, else the sandbox.
    fn met_in(&self, card: usize) -> Option<String> {
        let has = |kinds: Vec<Kind>| kinds.into_iter().any(|k| binder_card(k) == Some(card));
        let job = (0..self.jobs.len())
            .filter(|&i| self.unlocked(i))
            .find(|&i| has(self.jobs[i].drawer.iter().map(|it| it.kind).chain(self.jobs[i].board.parts.iter().map(|p| p.kind)).collect()));
        if let Some(i) = job {
            return Some(format!("First met: {}, for {}", self.jobs[i].name, self.jobs[i].customer));
        }
        let sandbox = if self.sandbox { Some(&self.board) } else { self.sandbox_board.as_ref() };
        sandbox.filter(|b| has(b.parts.iter().map(|p| p.kind).collect())).map(|_| "First met: in the sandbox".to_string())
    }

    fn binder_entries(&self) -> Vec<render::BinderEntry> {
        let shop = Job::sandbox().drawer;
        BINDER
            .iter()
            .enumerate()
            .map(|(i, &kind)| {
                let f = part_facts(kind);
                let values: Vec<String> = shop
                    .iter()
                    .filter(|it| binder_card(it.kind) == Some(i))
                    .filter_map(|it| match it.kind {
                        Kind::Led(c) => Some(format!("{} {:.1} V", c.name(), c.vf())),
                        Kind::Switch { .. } | Kind::Button { .. } | Kind::Ldr { .. } | Kind::Ntc { .. } => None,
                        k => part_name(k).split_once(' ').map(|(_, v)| v.replace("supply ", "")),
                    })
                    .collect();
                render::BinderEntry {
                    name: match kind {
                        Kind::Supply { .. } => "Bench supply".into(),
                        Kind::Switch { .. } => "Slide switch".into(),
                        Kind::Button { .. } => "Push button".into(),
                        _ => f.noun.into(),
                    },
                    collected: self.collected(i),
                    kind,
                    met: self.met_in(i).unwrap_or_default(),
                    values: values.iter().map(|v| v.replace(' ', "\u{a0}")).collect::<Vec<_>>().join(", "),
                    rules: f.rules.iter().map(|(k, r)| (k.to_string(), r.to_string())).collect(),
                    why: f.why.into(),
                    flavour: f.flavour.into(),
                }
            })
            .collect()
    }

    fn binder_act(&mut self, a: Act) {
        let Overlay::Binder { sel, scroll } = self.overlay else { return };
        let Some(l) = self.layout() else { return };
        let list = render::book_layout(&l).list;
        let n = BINDER.len();
        let to = match a {
            Act::Back | Act::Binder => {
                self.overlay = Overlay::None;
                return;
            }
            // The wheel over the list changes card; over the card it scrolls.
            Act::Scroll(x, y, up) if !render::in_rect(list, x, y) => {
                let s = if up { scroll.saturating_sub(1) } else { scroll + 1 };
                self.overlay = Overlay::Binder { sel, scroll: s };
                return;
            }
            Act::Move(Dir::N) | Act::PrevSlot | Act::Scroll(_, _, true) => (sel + n - 1) % n,
            Act::Move(Dir::S) | Act::NextSlot | Act::Scroll(_, _, false) => (sel + 1) % n,
            Act::Down(x, y, _) if render::in_rect(list, x, y) && ((y - list.y) as usize) < n => (y - list.y) as usize,
            Act::Down(x, y, _) if !render::in_rect(l.overlay, x, y) => {
                self.overlay = Overlay::None;
                return;
            }
            _ => sel,
        };
        if to != sel {
            self.audio.play(Sfx::Select);
            self.overlay = Overlay::Binder { sel: to, scroll: 0 };
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
                    let i = (y - book.list.y) as usize + render::list_top(self.pages.len(), page, book.list.height);
                    if i < self.pages.len() {
                        self.open_page(i);
                    }
                } else if let Some(p) = self.example_tile(x, y) {
                    // Flip a switch or press a button on the page's example circuit.
                    self.book_point = Some(p);
                    let Some((_, b, live)) = &mut self.example else { return };
                    if let Cell::Part(i) = b.cell(p) {
                        match b.parts[i].kind {
                            Kind::Switch { closed } => {
                                b.parts[i].kind = Kind::Switch { closed: !closed };
                                live.set_switch(i, !closed);
                            }
                            Kind::Button { held, nc } => {
                                b.parts[i].kind = Kind::Button { held: !held, nc };
                                live.set_switch(i, !held != nc);
                            }
                            // A sensor's light or warmth, or a pot's turn: the page's circuit
                            // is rebuilt with it.
                            k @ (Kind::Ldr { .. } | Kind::Ntc { .. } | Kind::Pot { .. }) => {
                                b.parts[i].kind = next_state(k);
                                let old = std::mem::replace(live, Live::build(b));
                                live.keep_charge(&old);
                            }
                            _ => return,
                        }
                        self.audio.play(Sfx::Click);
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
        if render::in_rect(render::help_button(self.area, &self.ledger_line()), x, y) {
            self.act(Act::Help);
            return;
        }
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
        if let Some(g) = render::title_at(&l, x, y) {
            self.act(Act::Grow(g));
            self.hover_title = render::title_at(&self.layout().unwrap_or(l), x, y);
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
        if render::in_rect(render::binder_button(&l), x, y) {
            self.act(Act::Binder);
            return;
        }
        if let Some(s) = render::slot_at(&l, x, y) {
            self.pick_slot(s);
            return;
        }
        let arrived = (0..self.jobs.len()).filter(|&i| self.unlocked(i)).count();
        if !self.sandbox && render::in_rect(l.inbox, x, y) && y > l.inbox.y {
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
                Cell::Part(i) if matches!(self.board.parts[i].kind, Kind::Switch { .. } | Kind::Ldr { .. } | Kind::Ntc { .. } | Kind::Pot { .. } | Kind::Npn { .. }) => self.operate(s.last),
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

    /// Notes every relay that moved in the last step: `moved` gets its new state.
    fn listen_to_relays(&mut self, moved: &mut Option<bool>) {
        self.relays.resize(self.board.parts.len(), false);
        for (i, p) in self.board.parts.iter().enumerate() {
            if matches!(p.kind, Kind::Relay { .. }) {
                let on = self.live.energised(i);
                if on != self.relays[i] {
                    self.relays[i] = on;
                    self.flips.push_back(self.live.sim.time);
                    *moved = Some(on);
                }
            }
        }
    }

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
        let mut moved = None;
        while self.acc >= STEP {
            self.acc -= STEP;
            if self.unsolvable {
                continue;
            }
            if self.live.sim.step(STEP).is_err() {
                self.unsolvable = true;
                self.say("The circuit can't be solved: two sources are fighting.");
            }
            self.listen_to_relays(&mut moved);
        }
        // A relay moving now and then clicks; one chattering on its own contact buzzes.
        while self.flips.front().is_some_and(|&t| t < self.live.sim.time - 0.25) {
            self.flips.pop_front();
        }
        let hz = self.flips.len() as f64 / 2.0 / 0.25;
        if hz >= 8.0 {
            self.audio.set_buzz(hz);
        } else {
            self.audio.set_buzz(0.0);
            if let Some(on) = moved {
                self.audio.play(if on { Sfx::RelayIn } else { Sfx::RelayOut });
            }
        }
        for i in 0..self.board.parts.len() {
            if self.live.reading(&self.board, i).burnt && !self.board.parts[i].burnt {
                self.board.parts[i].burnt = true;
                let label = self.board.parts[i].label.clone();
                let what = if matches!(self.board.parts[i].kind, Kind::Fuse { .. }) { "blew" } else { "burnt out" };
                self.say(format!("{label} {what}! Press f or click MEND to replace it."));
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
            Overlay::Binder { .. } => "binder",
        };
        log::debug!(
            "snap t={:.1} probe={:?} cursor={},{} pick={:?} facing={:?} copper={} parts={} amps={:.4} burnt={} overlay={overlay} grown={:?} done={} job={}",
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
            self.grown,
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
            _ if self.sandbox => "free".to_string(),
            _ => self.job.drawer.iter().find(|it| same_part(it.kind, kind)).map(|it| format!("{:.2} kr", it.price)).unwrap_or_default(),
        };
        let now = match i {
            Some(i) => {
                let r = live.reading(board, i);
                if r.burnt { "burnt out".to_string() } else { format!("{}  {}", si(r.volts, "V"), si(r.amps, "A")) }
            }
            None => String::new(),
        };
        let readings = match i {
            Some(i) if !board.parts[i].burnt && !matches!(kind, Kind::TestPoint) => {
                let r = live.reading(board, i);
                let mut v = vec![format!("{} across", si(r.volts, "V")), format!("{} through", si(r.amps, "A")), format!("{} power", si(r.watts, "W"))];
                if matches!(kind, Kind::Resistor { .. } | Kind::Led(_) | Kind::Relay { .. }) {
                    v.push(format!("{:.0}% of its rating", r.heat * 100.0));
                }
                if let Kind::Relay { nc, .. } = kind {
                    let on = live.energised(i);
                    v.insert(0, format!("{}, contact {}", if on { "pulled in" } else { "at rest" }, if on != nc { "closed" } else { "open" }));
                }
                v
            }
            _ => Vec::new(),
        };
        let side: [String; 3] = match kind {
            Kind::Resistor { ohms, watts } => [si(ohms, "Ω"), format!("{watts} W"), render::band_names(ohms)],
            Kind::Lamp { ohms, watts } => [si(ohms, "Ω") + " hot", format!("{watts} W"), String::new()],
            Kind::Battery { volts, r_int } => {
                let what = match volts {
                    v if (v - 1.5).abs() < 0.01 => "1 AA cell",
                    v if (v - 4.5).abs() < 0.01 => "3 AA cells",
                    v if (v - 9.0).abs() < 0.01 => "a 9 V block",
                    v if (v - 12.0).abs() < 0.01 => "a van battery",
                    _ => "",
                };
                [si(volts, "V"), format!("{} inside", si(r_int, "Ω")), what.into()]
            }
            Kind::Supply { volts, limit } => [si(volts, "V"), format!("limit {}", si(limit, "A")), String::new()],
            Kind::Switch { closed } => ["slide switch".into(), if closed { "on".into() } else { "off".into() }, String::new()],
            Kind::Button { held, nc } => [
                if nc { "stop button" } else { "push button" }.into(),
                if held { "held".into() } else { "up".into() },
                if nc { "breaks when pressed".into() } else { String::new() },
            ],
            Kind::Diode => ["1N4001".into(), "about 0.7 V".into(), "band to -".into()],
            Kind::Npn { .. } => ["BC547B, NPN".into(), "gain about 290".into(), "100 mA max".into()],
            Kind::Relay { volts, nc } => [
                format!("{} coil", value(volts, "V")),
                si(volts * volts / crate::solver::RELAY_WATTS, "Ω"),
                if nc { "normally closed".into() } else { "normally open".into() },
            ],
            Kind::Led(c) => [c.name().into(), format!("about {:.1} V", c.vf()), "30 mA max".into()],
            Kind::Capacitor { farads, volts } if kind.polarised() => [farads_name(farads), format!("{} max", value(volts, "V")), "stripe to -".into()],
            Kind::Capacitor { farads, .. } => [farads_name(farads), "ceramic".into(), "either way round".into()],
            Kind::TestPoint => ["test point".into(), String::new(), String::new()],
            Kind::Fuse { amps } => [format!("{} fast", si(amps, "A")), "carries its rating".into(), String::new()],
            Kind::Ldr { lux } => [light_name(lux).into(), format!("{lux} lux"), si(kind.sensor_ohms().unwrap(), "Ω")],
            Kind::Ntc { celsius } => [temp_name(celsius).into(), format!("{celsius} °C"), si(kind.sensor_ohms().unwrap(), "Ω")],
            Kind::Pot { ohms, turn } => [si(ohms, "Ω"), format!("turned {:.0}%", turn * 100.0), String::new()],
            Kind::Chip(c) => {
                let (vdd, vss) = c.supply();
                [format!("{}, CMOS", c.name()), "four NAND gates".into(), format!("+ on {vdd}, - on {vss}")]
            }
        };
        let f = part_facts(kind);
        let rules = f.rules.iter().map(|(k, r)| (k.to_string(), r.to_string())).collect();
        Some(Card { name: format!("{} {label}", f.noun), kind, price, side, now, rules, readings, why: f.why.into() })
    }

    fn slot_views(&self) -> Vec<Slot> {
        let mut v = vec![Slot { name: "Copper trace".into(), count: "free".into(), price: String::new(), kind: None }];
        for it in &self.drawer {
            let count = match it.count {
                Some(n) => format!("x{n}"),
                None => "any".into(),
            };
            let price = if it.price > 0.0 { format!("{:.2} kr", it.price) } else { String::new() };
            v.push(Slot { name: part_name(it.kind), count, price, kind: Some(it.kind) });
        }
        v
    }

    /// The ledger as the title bar shows it.
    fn ledger_line(&self) -> String {
        format!("ledger {:.2} kr", self.ledger)
    }

    pub fn draw(&self, buf: &mut Buffer) {
        self.draw_screen(buf);
        if let Some(scroll) = self.help
            && self.area.width >= render::MIN_W
            && self.area.height >= render::MIN_H
        {
            render::help(buf, self.area, scroll);
        }
    }

    fn draw_screen(&self, buf: &mut Buffer) {
        let area = self.area;
        if self.screen == Screen::Title {
            let line = format!("ledger {:.2} kr", self.ledger);
            let shelf = self.shelf_items();
            render::title(buf, area, &self.menu().into_iter().map(|m| m.1).collect::<Vec<_>>(), self.title_sel, &line, self.audio.muted, &shelf, self.shelf_page(), self.shelf_hover, self.time);
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
            Cell::Part(i) if matches!(scoped.parts[i].kind, Kind::Chip(_)) => format!("{} leg voltage", scoped.parts[i].label),
            Cell::Part(i) => format!("{} current", scoped.parts[i].label),
            Cell::Trace(_) | Cell::Jumper => "copper voltage".into(),
            _ => "nothing".into(),
        };
        let scope_unit = match scoped.cell(target) {
            Cell::Trace(_) | Cell::Jumper => "V",
            Cell::Part(i) if matches!(scoped.parts[i].kind, Kind::Chip(_)) => "V",
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
        } else if let Some(g) = self.hover_title {
            let title = g.title().to_lowercase();
            if self.grown == Some(g) {
                format!("click to put the {title} back · or {} · or Esc", g.key())
            } else {
                format!("click to enlarge the {title} · or {} · the board stays put", g.key())
            }
        } else if self.pick == Pick::Eraser {
            "eraser: click or drag to remove · space erases at the cursor · Esc or ERASE puts it down".to_string()
        } else if self.copper_mode {
            "arrows lay copper · c stops laying · x erases · Esc stops".to_string()
        } else if let Some(g) = self.grown {
            format!("{} or a click on its title puts the {} back · Esc too · i v o g enlarge a panel", g.key(), g.title().to_lowercase())
        } else {
            "arrows move · shift+arrows copper · space place · r rotate · x erase · e flip · t test · d drawers".to_string()
        };
        let d = DeskView {
            title: format!("{} · {}", self.job.name, self.job.customer),
            ledger: format!("{:.2} kr", self.ledger),
            // The sandbox's inbox is only its note.
            inbox: if self.sandbox {
                Vec::new()
            } else {
                self.jobs.iter().enumerate().filter(|&(i, _)| self.unlocked(i)).map(|(i, j)| (j.name.clone(), i == self.current, self.marks(i))).collect()
            },
            sealed: if self.sandbox { 0 } else { (0..self.jobs.len()).filter(|&i| !self.unlocked(i)).count() },
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
            goals: if self.sandbox { Vec::new() } else { self.goals(self.current, None).into_iter().skip(1).collect() },
            works: !self.sandbox && self.marks(self.current)[0],
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
                        .filter(|&i| !b.parts[i].kind.is_source())
                        .map(|i| {
                            let r = live.reading(b, i);
                            // A test point reads volts from minus; a pot, across its ends.
                            match b.parts[i].kind {
                                Kind::TestPoint => format!("{} {}", b.parts[i].label, si(r.volts, "V")),
                                _ => format!("{} {} {}", b.parts[i].label, si(r.volts, "V"), si(r.amps, "A")),
                            }
                        })
                        .collect::<Vec<_>>();
                    (b, live, flow, readings)
                });
                render::book(buf, &l, &titles, *page, &self.pages[*page], *scroll, example.as_ref().map(|(b, live, flow, r)| (*b, *live, flow.as_slice(), r.as_slice())), self.time);
            }
            Overlay::Report(r) => render::report(buf, &l, r),
            Overlay::Binder { sel, scroll } => render::binder(buf, &l, &self.binder_entries(), *sel, *scroll),
        }
    }

    fn overlay_is_none(&self) -> bool {
        matches!(self.overlay, Overlay::None)
    }
}

/// What a kind of part is, for its datasheet card and its page in the binder.
pub struct Facts {
    pub noun: &'static str,
    /// Keywords and their reminders.
    pub rules: &'static [(&'static str, &'static str)],
    /// Why it behaves as it does, in plain words.
    pub why: &'static str,
    /// The old master's line about it.
    pub flavour: &'static str,
}

pub fn part_facts(kind: Kind) -> Facts {
    let (noun, rules, why, flavour): (_, &'static [(&'static str, &'static str)], _, _) = match kind {
        Kind::Resistor { .. } => (
            "Resistor",
            &[("Ohm's law", "I = V / R"), ("Power", "P = V x I, all of it heat"), ("Rated", "past its watts it smokes, then burns open")],
            "It lets current through in proportion to the voltage across it, and turns the energy into heat. The colour bands give its value.",
            "The quiet one in every circuit. Read its bands before you trust it.",
        ),
        Kind::Lamp { .. } => (
            "Lamp",
            &[("Glows", "with power"), ("Filament", "a thin wire heated white-hot")],
            "Its filament's resistance rises as it heats. Here it is taken at its hot value, the one it works at.",
            "Old, hot and honest: if it glows, current flows.",
        ),
        Kind::Battery { .. } => (
            "Battery",
            &[("Source", "+ pushes out"), ("Inside", "its own resistance: the harder it works, the lower its voltage")],
            "Chemistry keeps one terminal at a higher voltage than the other. The resistance inside is why a short gets hot rather than drawing endless current.",
            "Fresh it reads a little high, tired a little low. Build for both.",
        ),
        Kind::Supply { .. } => (
            "Supply",
            &[("Limits", "its current"), ("Two modes", "holds its voltage until the limit, then holds the current instead")],
            "A bench supply: a short only draws the limit, so a mistake is a lesson rather than smoke.",
            "An apprentice's best friend. It would rather hold back than burn.",
        ),
        Kind::Switch { .. } => (
            "Switch",
            &[("Off", "breaks the loop"), ("On", "closes it, as good as copper")],
            "Two contacts held together or apart. Current needs the whole loop closed.",
            "Off is a gap in the loop, and a gap is all it takes.",
        ),
        Kind::Button { nc: true, .. } => (
            "Stop button",
            &[("Momentary", "while held"), ("Breaks", "closed at rest, open while pressed")],
            "A push button the other way round: its spring holds the contacts together, and pressing pulls them apart.",
            "Red, and it stops things. Every machine should have one.",
        ),
        Kind::Diode => (
            "Diode",
            &[
                ("One way", "current flows from anode to cathode (the band), never back"),
                ("Drop", "about 0.7 V across it while it conducts"),
                ("Rating", "1 A; much more and it cooks"),
            ],
            "A junction of two kinds of silicon. Pushed one way it lets current through once there is about 0.7 V across it; pushed the other way it blocks, as an open switch would.",
            "A one-way street. Put the band where the current comes out.",
        ),
        Kind::Npn { .. } => (
            "Transistor",
            &[
                ("Three legs", "collector, base, emitter: the emitter goes to -"),
                ("Gain", "the current into the base lets about 290 times as much into the collector"),
                ("Switch", "enough base current and it is fully on, a closed switch to the emitter"),
                ("Base", "needs about 0.65 V to start; always feed it through a resistor"),
                ("Rating", "100 mA through the collector, 0.625 W of heat"),
            ],
            "A small current from base to emitter opens the way for a big one from collector to emitter. Below about 0.6 V on the base nothing flows; above it the collector current follows the base current, many times over, until the transistor is fully on.",
            "A relay with no moving parts and no click, and far quicker.",
        ),
        Kind::Relay { .. } => (
            "Relay",
            &[
                ("Coil", "pulls in above 75% of its voltage, lets go below 5%"),
                ("Contact", "a switch the coil works: open at rest, or closed at rest (NC)"),
                ("Apart", "coil and contact share no copper: a small current switches a big one"),
                ("Time", "8 ms to pull in, 5 ms to let go"),
                ("Rating", "130% of its voltage for good; more cooks the coil"),
            ],
            "Current through the coil makes it a magnet, and the magnet pulls a springy contact over. Between pulling in and letting go there is a wide gap, so a relay stays as it is until the coil is clearly on or clearly off.",
            "It clicks. That's how you know it thought about it.",
        ),
        Kind::Chip(_) => (
            "Logic chip",
            &[
                ("NAND", "each gate's output is low only while both its inputs are high"),
                ("Power", "+ to pin 14, - to pin 7, 3 to 18 V; backwards it burns"),
                ("Inputs", "high above half the supply, low below; they take no current"),
                ("Never float", "an input wired to nothing keeps whatever it last had: give it a resistor to + or -"),
                ("Outputs", "+ or - through about 200 ohms: enough for an LED with its resistor"),
            ],
            "Inside are tiny switches, transistors, a pair for each input: one pair joins the output to + unless both inputs are high, the other joins it to - only when both are. Its inputs only feel voltage, so it can listen to a switch through 100 kΩ.",
            "Four little deciders in one black bug. Feed it, and never leave a leg hanging.",
        ),
        Kind::Button { .. } => (
            "Button",
            &[("Momentary", "while held"), ("Springs back", "open again when let go")],
            "A spring holds its contacts apart until a finger presses them together.",
            "On only while somebody's finger says so.",
        ),
        Kind::Led(_) => (
            "LED",
            &[("Polarised", "long leg to +"), ("Forward voltage", "dark below it, lit above it"), ("Needs a resistor", "nothing else limits its current")],
            "A diode that gives light. Past its forward voltage the current climbs steeply, so a little too much push is far too much current. A resistor in series sets it.",
            "Without its resistor, a very short-lived light.",
        ),
        Kind::Capacitor { .. } => (
            "Capacitor",
            &[
                ("Stores", "charge: it keeps a light going after the battery is gone"),
                ("Time", "through R, 63% charged (or 37% left) after R x C seconds"),
                ("Polarised", "from 1 µF up: the striped leg to -"),
                ("Rating", "holds its voltage and 15% more for a moment; above that, or backwards, it vents"),
            ],
            "Two plates with an insulator between. Current flows only while the voltage across it changes, and it takes time to change: the bigger the capacitor and the resistor feeding it, the longer.",
            "A bucket for charge. Mind which way round the big ones go.",
        ),
        Kind::TestPoint => (
            "Test point",
            &[("Measures", "volts here")],
            "A place for the bench's probe. It carries no current and changes nothing.",
            "Where the customer's meter goes.",
        ),
        Kind::Fuse { .. } => (
            "Fuse",
            &[("Carries", "its rating, and half again for an hour or more"), ("Opens", "past twice its rating: at 4 times within 0.3 s, on a short in milliseconds"), ("Mend", "puts a new one in")],
            "A thin wire that melts when too much current heats it. It is the weakest link on purpose, so the wiring isn't.",
            "Pick the smallest that holds the load. Bigger protects nothing.",
        ),
        Kind::Ldr { .. } => (
            "Light sensor",
            &[("Light", "lowers its resistance: about 15 kΩ in a dim room, 600 Ω in daylight"), ("Dark", "a megohm or more"), ("Operate", "e or a click changes the light on it")],
            "A cadmium sulphide cell: light frees charges in it, and the more light, the more of them carry current.",
            "On its own it measures nothing. Put it in a divider.",
        ),
        Kind::Ntc { .. } => (
            "Thermistor",
            &[("Warmth", "lowers its resistance: 10 kΩ at 25 °C, about 27 kΩ in a fridge"), ("Operate", "e or a click changes its temperature")],
            "NTC: negative temperature coefficient. Heat frees charges in its ceramic, so it conducts better as it warms.",
            "A thermometer with no numbers on it until you build them.",
        ),
        Kind::Pot { .. } => (
            "Potentiometer",
            &[("Three legs", "the two ends are the whole track, the middle leg the wiper"), ("Divider", "ends across the supply: the wiper gives anything between"), ("Variable", "one end and the wiper: a resistor you can turn"), ("Turn", "the wheel or e")],
            "A resistive track with a contact sliding along it: the wiper splits the track into two resistors that always add up to the whole.",
            "Turn it all the way before you trust it. Both ends.",
        ),
    };
    Facts { noun, rules, why, flavour }
}

/// What operating a sensor or a pot makes of it: an LDR steps up through the light, a
/// thermistor through the temperatures, a pot a tenth of a turn; each goes round again at the
/// end.
fn next_state(k: Kind) -> Kind {
    match k {
        Kind::Ldr { lux } => Kind::Ldr { lux: LUX_STEPS.iter().copied().find(|&l| l > lux).unwrap_or(LUX_STEPS[0]) },
        Kind::Ntc { celsius } => Kind::Ntc { celsius: CELSIUS_STEPS.iter().copied().find(|&c| c > celsius).unwrap_or(CELSIUS_STEPS[0]) },
        Kind::Pot { ohms, turn } => {
            let next = ((turn + 0.1) * 10.0).round() / 10.0;
            Kind::Pot { ohms, turn: if next > 1.0 + 1e-9 { 0.0 } else { next } }
        }
        k => k,
    }
}

/// How an LDR's light reads in words.
fn light_name(lux: f64) -> &'static str {
    match lux {
        l if l <= 0.0 => "pitch dark",
        l if l <= 1.0 => "moonlight",
        l if l <= 10.0 => "a dim room",
        l if l <= 100.0 => "a lit room",
        _ => "daylight",
    }
}

/// How a thermistor's temperature reads in words.
fn temp_name(c: f64) -> &'static str {
    match c {
        c if c <= -10.0 => "freezer",
        c if c <= 8.0 => "fridge",
        c if c <= 25.0 => "room",
        _ => "a warm hand",
    }
}

/// Builds the bike lamp by keyboard, as a player would: 220 ohm along the top, the switch along
/// the bottom. For tests and the scripted shots.
/// Puts the old master's solution for the job on the desk in place of the board, for shots.
pub fn show_master(a: &mut App) {
    let (board, _) = a.job.master.embed(BENCH.0, BENCH.1);
    a.board = board;
    a.rebuilt();
}

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
        Cell::Part(i) if matches!(b.parts[i].kind, Kind::Chip(_)) => {
            // A chip: the leg under the pointer, from ground, and the package's supply.
            let Kind::Chip(chip) = b.parts[i].kind else { unreachable!() };
            let r = live.reading(b, i);
            let label = &b.parts[i].label;
            let leg = b.parts[i].pin_ports().iter().position(|(p, _)| *p == at);
            let mut more = vec![("supply".into(), si(r.volts, "V")), ("draws".into(), si(r.amps, "A"))];
            let high = live.gates_high(i);
            for (g, &(_, _, y)) in chip.gates().iter().enumerate() {
                more.push((format!("gate {} (pin {y})", (b'A' + g as u8) as char), if high.get(g) == Some(&true) { "high".into() } else { "low".into() }));
            }
            if r.burnt {
                more.push(("state".into(), "burnt out".into()));
            }
            match leg {
                Some(k) => {
                    let v = live.pin_volts(i, k);
                    (v, Meter { value: v, unit: "V", what: format!("{label} {}", chip.pin_name(k + 1)), second: String::new(), more })
                }
                None => (r.volts, Meter { value: r.volts, unit: "V", what: format!("{label} supply"), second: String::new(), more }),
            }
        }
        Cell::Part(i) => {
            let r = live.reading(b, i);
            let label = &b.parts[i].label;
            let what = if b.parts[i].kind.pins() == 1 { format!("{label} to ground") } else { format!("across {label}") };
            let two = b.parts[i].kind.pins() == 2;
            let second = if two { format!("{} through", si(r.amps, "A")) } else { String::new() };
            let mut more = Vec::new();
            if two {
                more.push(("current".into(), si(r.amps, "A")));
                more.push(("power".into(), si(r.watts, "W")));
                if r.amps.abs() > 1e-9 && !b.parts[i].kind.is_source() {
                    more.push(("V / I".into(), si(r.volts / r.amps, "Ω")));
                }
                if matches!(b.parts[i].kind, Kind::Resistor { .. } | Kind::Led(_)) {
                    more.push(("load".into(), format!("{:.0}% of rating", r.heat * 100.0)));
                }
            }
            if r.burnt {
                more.push(("state".into(), "burnt out".into()));
            }
            (r.amps, Meter { value: r.volts, unit: "V", what, second, more })
        }
        Cell::Trace(m) => {
            let d = Dir::ALL.into_iter().find(|d| m & d.bit() != 0).unwrap_or(Dir::N);
            let v = live.net.net_at(b, at, d).map(|n| live.net_volts(n)).unwrap_or(0.0);
            let more = vec![("copper".into(), "one voltage".into())];
            (v, Meter { value: v, unit: "V", what: "copper to ground".into(), second: String::new(), more })
        }
        Cell::Jumper => {
            let v = live.net.net_at(b, at, Dir::E).map(|n| live.net_volts(n)).unwrap_or(0.0);
            (v, Meter { value: v, unit: "V", what: "jumper, east-west".into(), second: String::new(), more: Vec::new() })
        }
        _ => (0.0, Meter { value: 0.0, unit: "V", what: "nothing here".into(), second: String::new(), more: Vec::new() }),
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
    fn a_grown_panel_takes_its_strip_and_the_board_stays_put() {
        let mut a = app();
        for size in [(100, 30), (140, 45)] {
            a.area = Rect::new(0, 0, size.0, size.1);
            let plain = a.layout().unwrap();
            for g in Grow::ALL {
                a.grown = Some(g);
                let l = a.layout().unwrap();
                assert_eq!((l.board, l.inner, l.view, l.origin), (plain.board, plain.inner, plain.view, plain.origin), "{g:?}");
                let rects = [l.inbox, l.board, l.drawers, l.meter, l.scope, l.card];
                for (i, r) in rects.iter().enumerate() {
                    assert!(r.x + r.width <= a.area.width && r.y + r.height < a.area.height, "{g:?} {i} off screen");
                    for q in &rects[i + 1..] {
                        assert!(r.intersection(*q).is_empty(), "{g:?}: {r:?} overlaps {q:?}");
                    }
                }
                for s in &l.slots {
                    assert!(s.y + s.height < l.drawers.y + l.drawers.height, "{g:?}: a drawer slot spills out");
                }
            }
            a.grown = None;
        }
        a.area = Rect::new(0, 0, 100, 30);
        a.grown = Some(Grow::Scope);
        let l = a.layout().unwrap();
        assert_eq!(l.scope.width, 100);
        assert_eq!((l.meter.height, l.card.height), (1, 1));
        a.grown = Some(Grow::Card);
        let l = a.layout().unwrap();
        assert_eq!(l.drawers.height, 1);
        // Down to the hint row, as the drawers and datasheet reached together.
        assert_eq!(l.card.y + l.card.height, a.area.height - 1);
    }

    #[test]
    fn clicking_a_title_grows_its_panel_and_esc_puts_it_back() {
        let mut a = app();
        a.area = Rect::new(0, 0, 100, 30);
        let l = a.layout().unwrap();
        a.act(Act::Down(l.scope.x + 4, l.scope.y, false));
        assert_eq!(a.grown, Some(Grow::Scope));
        // The folded multimeter's title swaps to it.
        let l = a.layout().unwrap();
        assert_eq!(l.meter.height, 1);
        a.act(Act::Down(l.meter.x + 4, l.meter.y, false));
        assert_eq!(a.grown, Some(Grow::Meter));
        // Its own title again puts it back.
        let l = a.layout().unwrap();
        a.act(Act::Down(l.meter.x + 4, l.meter.y, false));
        assert_eq!(a.grown, None);
        // The key, then Esc: back to the desk as it was, not to the launch page.
        a.act(Act::Grow(Grow::Card));
        a.act(Act::Back);
        assert_eq!(a.grown, None);
        assert_eq!(a.screen, Screen::Desk);
        // While a panel is grown the edges can't be dragged.
        a.act(Act::Grow(Grow::Inbox));
        let l = a.layout().unwrap();
        assert_eq!(render::edge_at(&l, l.board.x, l.board.y + 3), None);
    }

    #[test]
    fn f1_opens_help_anywhere_and_it_scrolls_to_its_end_and_closes() {
        let mut a = app();
        a.area = Rect::new(0, 0, 100, 30);
        // On the launch page, by its menu item.
        a.screen = Screen::Title;
        assert!(a.menu().iter().any(|m| m.0 == MenuItem::Help));
        a.choose(MenuItem::Help);
        assert_eq!(a.help, Some(0));
        a.act(Act::Back);
        assert_eq!((a.help, a.screen), (None, Screen::Title));
        // On the desk: keys don't reach the board while it is open.
        a.screen = Screen::Desk;
        let cursor = a.cursor;
        a.act(Act::Help);
        a.act(Act::Move(Dir::E));
        assert_eq!(a.cursor, cursor);
        assert_eq!(a.help, Some(0));
        // It scrolls as far as there is more, and no further.
        for _ in 0..200 {
            a.act(Act::Move(Dir::S));
        }
        let end = a.help.unwrap();
        assert!(end > 0 && !render::help_more(a.area, end));
        a.act(Act::NextJob);
        assert_eq!(a.help, Some(end));
        a.act(Act::PrevJob);
        assert!(a.help.unwrap() < end);
        a.act(Act::Help);
        assert_eq!((a.help, a.screen), (None, Screen::Desk));
        // The label in the title bar opens it too.
        let b = render::help_button(a.area, &a.ledger_line());
        a.act(Act::Down(b.x + 1, b.y, false));
        assert!(a.help.is_some());
        // A terminal wide enough shows it all at once.
        a.area = Rect::new(0, 0, 160, 45);
        assert!(!render::help_more(a.area, 0));
    }

    #[test]
    fn the_sandbox_has_every_kind_of_part_free_and_keeps_both_boards() {
        let mut a = app();
        // From the very start, every kind the simulator knows, unlimited and free.
        a.enter_sandbox();
        assert!(a.sandbox);
        let kinds = |a: &App| a.drawer.iter().map(|it| std::mem::discriminant(&it.kind)).collect::<Vec<_>>();
        for k in [
            Kind::Resistor { ohms: 1.0, watts: 1.0 },
            Kind::Lamp { ohms: 1.0, watts: 1.0 },
            Kind::Battery { volts: 1.0, r_int: 1.0 },
            Kind::Supply { volts: 1.0, limit: 1.0 },
            Kind::Switch { closed: false },
            Kind::Button { held: false, nc: false },
            Kind::Led(crate::circuit::Colour::Red),
            Kind::Capacitor { farads: 1.0, volts: 1.0 },
        ] {
            assert!(kinds(&a).contains(&std::mem::discriminant(&k)), "{k:?} missing");
        }
        assert!(a.drawer.iter().all(|it| it.count.is_none() && it.price == 0.0));
        let names: Vec<String> = a.drawer.iter().map(|it| part_name(it.kind)).collect();
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), names.len(), "two drawers read the same: {names:?}");
        // Build in it, then leave for the job: each keeps its own board.
        let p = a.at(3, 3);
        a.cursor = p;
        a.pick_slot(1);
        a.act(Act::Place);
        assert!(matches!(a.board.cell(p), Cell::Part(_)));
        a.act(Act::Test);
        assert!(matches!(a.overlay, Overlay::None), "nothing to test in the sandbox");
        a.act(Act::NextJob);
        assert!(a.sandbox, "PgDn doesn't leave the sandbox");
        a.screen = Screen::Title;
        a.choose(MenuItem::Continue);
        assert!(!a.sandbox && a.job.id == "bike-lamp");
        assert!(!matches!(a.board.cell(p), Cell::Part(_)), "the job's board is its own");
        a.choose(MenuItem::Sandbox);
        assert!(matches!(a.board.cell(p), Cell::Part(_)), "the sandbox board came back");
    }

    #[test]
    fn the_binder_opens_on_the_part_at_hand_and_collects_as_parts_are_met() {
        let mut a = app();
        let got = |a: &App| (0..BINDER.len()).filter(|&i| a.collected(i)).count();
        // The bike lamp brings a battery, an LED, resistors and a switch.
        assert_eq!(got(&a), 4);
        build_bike_lamp(&mut a);
        let r = a.board.part("R1").unwrap();
        a.cursor = a.board.parts[r].pos;
        a.act(Act::Binder);
        let Overlay::Binder { sel, .. } = a.overlay else { panic!("no binder") };
        assert_eq!(BINDER[sel], Kind::Resistor { ohms: 220.0, watts: 0.25 });
        a.act(Act::Move(Dir::S));
        assert!(matches!(a.overlay, Overlay::Binder { sel: s, .. } if s == sel + 1));
        a.act(Act::Binder);
        assert!(matches!(a.overlay, Overlay::None));
        // A capacitor on the sandbox board collects its card; the porch lamp brings its lamp
        // and the push button.
        a.enter_sandbox();
        let cap = a.drawer.iter().position(|it| matches!(it.kind, Kind::Capacitor { .. })).unwrap();
        a.cursor = a.at(2, 2);
        a.pick_slot(cap + 1);
        a.act(Act::Place);
        assert_eq!(got(&a), 5);
        a.solved[0] = true;
        assert_eq!(got(&a), 7);
        let cards = a.binder_entries();
        assert!(cards.iter().filter(|c| c.collected).all(|c| !c.met.is_empty() && !c.flavour.is_empty()));
    }

    #[test]
    fn sensors_and_pots_change_when_operated_and_the_wheel_turns_a_pot() {
        let mut a = app();
        a.enter_sandbox();
        let slot = |a: &App, f: &dyn Fn(Kind) -> bool| a.drawer.iter().position(|it| f(it.kind)).unwrap() + 1;
        let ldr = slot(&a, &|k| matches!(k, Kind::Ldr { .. }));
        let pot = slot(&a, &|k| matches!(k, Kind::Pot { ohms, .. } if ohms == 10e3));
        let (p, q) = (a.at(2, 2), a.at(6, 2));
        a.cursor = p;
        a.pick_slot(ldr);
        a.act(Act::Place);
        a.cursor = q;
        a.pick_slot(pot);
        a.act(Act::Place);
        let kind = |a: &App, at: Pos| match a.board.cell(at) {
            Cell::Part(i) => a.board.parts[i].kind,
            _ => panic!("no part"),
        };
        a.cursor = p;
        a.act(Act::Operate);
        assert_eq!(kind(&a, p), Kind::Ldr { lux: 1000.0 });
        a.act(Act::Operate);
        assert_eq!(kind(&a, p), Kind::Ldr { lux: 0.0 }, "round again after daylight");
        a.cursor = q;
        a.act(Act::Operate);
        assert!(matches!(kind(&a, q), Kind::Pot { turn, .. } if (turn - 0.6).abs() < 1e-9));
        let l = a.layout().unwrap();
        let (x, y) = (l.origin.0 + ((q.x - l.view.0) as u16) * render::TILE_W + 2, l.origin.1 + ((q.y - l.view.1) as u16) * render::TILE_H + 1);
        for _ in 0..10 {
            a.act(Act::Scroll(x, y, true));
        }
        assert!(matches!(kind(&a, q), Kind::Pot { turn, .. } if turn == 1.0), "the wheel stops at the end");
        assert_eq!(a.board.parts.iter().find(|pt| pt.pos == q).unwrap().facing, a.facing, "the wheel turned the knob, not the part");
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
    fn a_gadget_on_the_shelf_opens_its_job_and_a_sealed_one_stays_shut() {
        let mut a = app();
        a.screen = Screen::Title;
        a.solved[0] = true;
        let slots = render::shelf_slots(a.area, a.jobs.len(), 0);
        // The porch lamp has arrived (the bike lamp is solved); the doorbells haven't.
        a.act(Act::Down(slots[2].x + 2, slots[2].y, false));
        assert_eq!(a.screen, Screen::Title);
        a.act(Act::Down(slots[1].x + 2, slots[1].y, false));
        assert_eq!(a.screen, Screen::Desk);
        assert_eq!(a.current, 1);
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

    /// Builds a job's master solution through the desk, as a player would: each part picked
    /// from its drawer, turned with r, placed, bent with e if the master's is; every jumper laid
    /// as a crossing and made one with e; then the copper, tile by tile.
    fn build_master(a: &mut App, job: usize) -> Result<(), String> {
        for k in 0..job {
            a.solved[k] = true;
        }
        a.open_job(job);
        let master = a.jobs[job].master.clone();
        let fixed: Vec<Pos> = a.jobs[job].board.parts.iter().map(|p| p.pos).collect();
        for p in &master.parts {
            if fixed.contains(&p.pos) {
                continue;
            }
            let slot = a
                .drawer
                .iter()
                .position(|it| crate::job::same_part(it.kind, p.kind) && it.count != Some(0))
                .ok_or(format!("{}: no {} left in the drawers", p.label, crate::job::kind_words(p.kind)))?;
            a.act(Act::Slot(slot + 1));
            let at = a.at(p.pos.x, p.pos.y);
            a.cursor = at;
            for _ in 0..4 {
                if a.facing == p.facing {
                    break;
                }
                a.act(Act::Rotate);
            }
            a.act(Act::Place);
            if !matches!(a.board.cell(at), Cell::Part(_)) {
                return Err(format!("{} wouldn't go down at {:?}", p.label, p.pos));
            }
            if let Kind::Npn { flip: true } = p.kind {
                a.act(Act::Operate);
            }
        }
        a.act(Act::Slot(0));
        for y in 0..master.h {
            for x in 0..master.w {
                if master.cell(Pos { x, y }) == Cell::Jumper {
                    for d in Dir::ALL {
                        a.cursor = a.at(x, y);
                        a.act(Act::Lay(d));
                    }
                    a.cursor = a.at(x, y);
                    a.act(Act::Operate);
                }
            }
        }
        for y in 0..master.h {
            for x in 0..master.w {
                if let Cell::Trace(m) = master.cell(Pos { x, y }) {
                    for d in Dir::ALL.into_iter().filter(|d| m & d.bit() != 0) {
                        a.cursor = a.at(x, y);
                        a.act(Act::Lay(d));
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    fn every_masters_solution_can_be_built_at_the_desk_and_passes() {
        for job in 0..crate::job::JOBS.len() {
            let mut a = app();
            if let Err(e) = build_master(&mut a, job) {
                panic!("{}: {e}", a.jobs[job].name);
            }
            // The desk's board, cut back to the job's own area, is the master's to the tile.
            let m = &a.jobs[job].master;
            for y in 0..m.h {
                for x in 0..m.w {
                    let (got, want) = (a.board.cell(a.at(x, y)), m.cell(Pos { x, y }));
                    let same = match (got, want) {
                        (Cell::Part(i), Cell::Part(j)) => {
                            let (p, q) = (&a.board.parts[i], &m.parts[j]);
                            // A sensor's light or a switch's state is the bench's to set; a
                            // transistor's bent legs are the builder's.
                            let flip = |k: Kind| matches!(k, Kind::Npn { flip: true });
                            p.facing == q.facing && crate::job::same_part(p.kind, q.kind) && flip(p.kind) == flip(q.kind)
                        }
                        (g, w) => g == w,
                    };
                    assert!(same, "{}: tile {x},{y} is {got:?}, the master's {want:?}", a.jobs[job].name);
                }
            }
            a.update(0.2);
            a.act(Act::Test);
            let Overlay::Report(r) = &a.overlay else { panic!("{}: no report", a.jobs[job].name) };
            assert!(r.pass, "{}: {:?}", a.jobs[job].name, r.rows);
            assert!(r.goals[1].1, "{}: the master's own build misses the parts tick", a.jobs[job].name);
        }
    }
}
