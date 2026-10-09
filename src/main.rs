//! Ohm's Workshop: an electronics desk puzzle with a real circuit solver.
//!
//! Run:
//!   cargo run --release                          # play (--mute for no sound)
//!   RUST_LOG=debug cargo run --release 2>ohm.log # with a session log
//!   cargo run --release -- --bench               # the printed bench: numbers and hints as text
//!   cargo run --release -- --shot desk [--demo] [--secs S] [--overlay drawers|report|bookN]
//!                                [--cursor X,Y] (a tile of the job's board) [--job N] [--cells]
//!   cargo run --release -- --shot title --cells | python3 tools/shot.py title.png
//!   cargo run --release -- --shot gallery --cells | python3 tools/shot.py gallery.png
//!   cargo test

mod app;
mod audio;
mod bench;
mod book;
mod circuit;
mod input;
mod job;
mod printout;
mod progress;
mod render;
mod solver;

use std::io;
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, DisableFocusChange, DisableMouseCapture, EnableFocusChange, EnableMouseCapture};
use ratatui::crossterm::execute;
use ratatui::layout::Rect;

use app::App;
use audio::Audio;
use input::{Act, Input};
use job::Job;

const FRAME: Duration = Duration::from_millis(16);

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str);
    let flag = |name: &str| args.iter().any(|a| a == name);
    if flag("--bench") {
        printout::run();
        return Ok(());
    }
    if let Some(screen) = arg("--shot") {
        let size = arg("--size").and_then(|s| s.split_once('x')).and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)));
        let secs = arg("--secs").and_then(|s| s.parse().ok()).unwrap_or(1.0);
        let job = arg("--job").and_then(|s| s.parse().ok());
        let cursor = arg("--cursor").and_then(|s| s.split_once(',')).and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?)));
        shot(screen, size, flag("--demo"), secs, arg("--overlay"), cursor, job, flag("--cells"));
        return Ok(());
    }
    env_logger::Builder::from_default_env().format_timestamp_millis().init();
    log::info!("Ohm's Workshop starting");
    let jobs = (0..job::JOBS.len()).map(Job::load).collect();
    let progress = progress::Progress::load();
    let audio = Audio::new(flag("--mute") || progress.muted);
    let mut app = App::new(jobs, progress, audio);
    let mut terminal = ratatui::init();
    execute!(io::stdout(), EnableMouseCapture, EnableFocusChange)?;
    let result = run(&mut terminal, &mut app);
    app.save();
    let _ = execute!(io::stdout(), DisableMouseCapture, DisableFocusChange);
    ratatui::restore();
    log::info!("Ohm's Workshop quit");
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> io::Result<()> {
    let mut input = Input::default();
    let mut last = Instant::now();
    let mut last_log = Instant::now();
    while !app.quit {
        let deadline = last + FRAME;
        while event::poll(deadline.saturating_duration_since(Instant::now()))? {
            input.on_event(event::read()?);
        }
        for a in std::mem::take(&mut input.acts) {
            app.act(a);
        }
        let now = Instant::now();
        app.update(now.duration_since(last).as_secs_f64());
        last = now;
        if now.duration_since(last_log) >= Duration::from_millis(500) {
            app.snapshot_log();
            last_log = now;
        }
        terminal.draw(|f| {
            app.area = f.area();
            app.draw(f.buffer_mut());
        })?;
    }
    Ok(())
}

/// Draws a screen into an off-screen buffer and prints it: as text, or with `cells` one line per
/// cell (`x y fg bg bold symbol`, colours as hex) for `tools/shot.py`. `demo` builds the first
/// job's solution the way a player would before the clock runs for `secs`.
#[allow(clippy::too_many_arguments)]
fn shot(screen: &str, size: Option<(u16, u16)>, demo: bool, secs: f64, overlay: Option<&str>, cursor: Option<(usize, usize)>, job: Option<usize>, cells: bool) {
    let (w, h) = size.unwrap_or(match screen {
        "gallery" => (104, 62),
        _ => (100, 30),
    });
    let area = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(area);
    if screen == "gallery" {
        render::gallery(&mut buf, area);
    } else {
        let jobs = (0..job::JOBS.len()).map(Job::load).collect();
        // --saved starts from the save file (point XDG_DATA_HOME at a scratch one), else afresh.
        let progress = if std::env::args().any(|a| a == "--saved") { progress::Progress::load() } else { progress::Progress::default() };
        let mut app = App::new(jobs, progress, Audio::new(true));
        app.area = area;
        app.screen = if screen == "title" { app::Screen::Title } else { app::Screen::Desk };
        if let Some(j) = job {
            // As if every job before it were solved.
            for k in 0..j {
                app.solved[k] = true;
            }
            app.open_job(j);
        }
        if demo {
            app::build_bike_lamp(&mut app);
            app.cursor = app.at(8, 2);
            app.act(Act::Probe);
            app.cursor = app.at(4, 1);
        }
        let mut t = 0.0;
        while t < secs {
            app.update(1.0 / 60.0);
            t += 1.0 / 60.0;
        }
        if let Some((x, y)) = cursor {
            app.cursor = app.at(x, y);
        }
        match overlay {
            Some("drawers") => app.act(Act::Drawer),
            Some("report") => app.act(Act::Test),
            Some(page) if page.starts_with("book") => {
                app.act(Act::Notebook);
                for _ in 0..page[4..].parse::<usize>().unwrap_or(0) {
                    app.act(Act::Move(circuit::Dir::S));
                }
                for _ in 0..30 {
                    app.update(1.0 / 60.0);
                }
            }
            _ => {}
        }
        app.draw(&mut buf);
    }
    if cells {
        let hex = |c: ratatui::style::Color| match c {
            ratatui::style::Color::Rgb(r, g, b) => format!("{r:02x}{g:02x}{b:02x}"),
            _ => "000000".into(),
        };
        println!("{w} {h}");
        for y in 0..h {
            for x in 0..w {
                let c = &buf[(x, y)];
                let bold = c.modifier.contains(ratatui::style::Modifier::BOLD) as u8;
                println!("{x} {y} {} {} {bold} {}", hex(c.fg), hex(c.bg), c.symbol().escape_unicode());
            }
        }
        return;
    }
    for y in 0..h {
        let row: String = (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect();
        println!("{}", row.trim_end());
    }
}
