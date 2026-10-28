//! Raw terminal events and gamepads into `Act`s. Every raw event is logged before it is mapped
//! (the Asciiverse session-logging convention). Mouse and keyboard are equals: everything can be
//! done with either, and a key acts on whatever tile the pointer last moved the cursor to.
//!
//! A gamepad plays the whole game too: d-pad or left stick moves (repeating while held), A
//! places (holding A while moving lays copper), B erases (backs out of a page), X rotates, Y
//! operates, the shoulders change drawer, the triggers undo and redo, Start runs the test bench
//! and Back (View) backs out. Holding Back makes the face buttons, shoulders and triggers reach
//! the rest; see `layer`. Pads are ignored while the terminal reports it has lost focus, and
//! only pads in SDL's controller database are used (`OHM_ANY_PAD=1` takes all), as in SIGPIPE.

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::circuit::Dir;
use crate::render::Grow;
#[cfg(feature = "gamepad")]
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Act {
    Move(Dir),
    /// Lay copper from the cursor one tile on, and move there.
    Lay(Dir),
    /// Place the picked part, or pick in an overlay.
    Place,
    Rotate,
    /// Erase what's at the cursor.
    Erase,
    /// Pick up or put down the eraser tool.
    Eraser,
    /// Flip a switch, press a button, turn a junction into a jumper and back.
    Operate,
    Test,
    Drawer,
    /// Open or close the Master's Notebook.
    Notebook,
    /// Pin the scope's probe to the tile under the cursor, or take it off; from the mouse,
    /// the next board click pins it.
    Probe,
    /// The middle mouse button went down: a click pins the probe, a drag pans the board.
    MiddleDown(u16, u16),
    MiddleDrag(u16, u16),
    /// Pick drawer slot n (0: copper).
    Slot(usize),
    NextSlot,
    PrevSlot,
    CopperMode,
    /// Open or close the help page.
    Help,
    /// Open or close the card binder.
    Binder,
    /// Grow a panel along its strip, or put it back.
    Grow(Grow),
    /// The next or previous job in the inbox.
    NextJob,
    PrevJob,
    Mend,
    /// Erase on the desk; back out anywhere else (a pad's B).
    #[cfg_attr(not(feature = "gamepad"), allow(dead_code))]
    Cancel,
    Undo,
    Redo,
    Mute,
    Back,
    Quit,
    /// The pointer moved to x, y.
    Hover(u16, u16),
    /// A mouse button went down; `right` for the right one.
    Down(u16, u16, bool),
    /// Moved with a button held.
    Drag(u16, u16, bool),
    Up(u16, u16),
    Scroll(u16, u16, bool),
}

/// A held pad direction repeats after this long, then every `REPEAT_EVERY`.
#[cfg(feature = "gamepad")]
const REPEAT_AFTER: Duration = Duration::from_millis(260);
#[cfg(feature = "gamepad")]
const REPEAT_EVERY: Duration = Duration::from_millis(90);
#[cfg(feature = "gamepad")]
const STICK_DEADZONE: f32 = 0.5;

pub struct Input {
    pub acts: Vec<Act>,
    /// The terminal has focus (as far as it tells us).
    #[cfg_attr(not(feature = "gamepad"), allow(dead_code))]
    focused: bool,
    #[cfg(feature = "gamepad")]
    pad: Pad,
}

/// What the pads are holding down.
#[cfg(feature = "gamepad")]
#[derive(Default)]
struct Pad {
    gilrs: Option<gilrs::Gilrs>,
    dpad: Option<Dir>,
    stick: Option<Dir>,
    /// The direction held and when it next repeats.
    held: Option<(Dir, Instant)>,
    /// A is down, and whether copper has been laid since it went down (then letting go
    /// doesn't place).
    a: Option<bool>,
    /// Back is down, and whether it has been used as a layer since (then letting go doesn't
    /// back out).
    back: Option<bool>,
}

#[cfg(feature = "gamepad")]
impl Pad {
    /// Forget everything held (focus lost, a pad gone).
    fn let_go(&mut self) {
        (self.dpad, self.stick, self.held, self.a, self.back) = (None, None, None, None, None);
    }
}

/// A pad event, apart from the pad, so the mapping can be tested.
#[cfg(feature = "gamepad")]
enum PadEvent {
    Button(gilrs::Button, bool),
    /// The left stick, x right and y down.
    Stick((f32, f32)),
}

#[cfg(feature = "gamepad")]
fn usable_pad(p: &gilrs::Gamepad) -> bool {
    p.mapping_source() == gilrs::MappingSource::SdlMappings || std::env::var("OHM_ANY_PAD").is_ok_and(|v| v == "1")
}

/// A button with Back held: the rest of the desk.
#[cfg(feature = "gamepad")]
fn layer(b: gilrs::Button) -> Option<Act> {
    use gilrs::Button;
    Some(match b {
        Button::South => Act::Drawer,
        Button::East => Act::Binder,
        Button::West => Act::Notebook,
        Button::North => Act::Help,
        Button::LeftTrigger => Act::PrevJob,
        Button::RightTrigger => Act::NextJob,
        Button::LeftTrigger2 => Act::Mend,
        Button::RightTrigger2 => Act::Probe,
        Button::Start => Act::Mute,
        _ => return None,
    })
}

impl Default for Input {
    fn default() -> Input {
        Input {
            acts: Vec::new(),
            focused: true,
            #[cfg(feature = "gamepad")]
            pad: Pad {
                gilrs: match gilrs::Gilrs::new() {
                    Ok(g) => {
                        let all: Vec<_> = g.gamepads().map(|(_, p)| (p.name().to_string(), usable_pad(&p))).collect();
                        log::info!("gamepads at start (name, used): {all:?}");
                        Some(g)
                    }
                    Err(e) => {
                        log::warn!("gamepad support unavailable: {e}");
                        None
                    }
                },
                ..Pad::default()
            },
        }
    }
}

impl Input {
    pub fn on_event(&mut self, ev: Event) {
        match ev {
            Event::Key(k) => self.key(k),
            Event::Mouse(m) => self.mouse(m),
            Event::FocusGained | Event::FocusLost => {
                self.focused = ev == Event::FocusGained;
                log::info!("focus {}", if self.focused { "gained" } else { "lost: pads ignored" });
            }
            _ => {}
        }
    }

    /// Drain gamepad events and repeat a held direction (call once per frame).
    #[cfg_attr(not(feature = "gamepad"), allow(unused_variables))]
    pub fn poll_pads(&mut self, now: std::time::Instant) {
        #[cfg(feature = "gamepad")]
        {
            use gilrs::{Axis, EventType};
            let Some(g) = &mut self.pad.gilrs else { return };
            let mut events = Vec::new();
            while let Some(ev) = g.next_event() {
                let pad = g.gamepad(ev.id);
                if !usable_pad(&pad) {
                    continue;
                }
                let stick = || {
                    let axis = |a: Axis| pad.axis_data(a).map_or(0.0, |d| d.value());
                    // stick up is positive; screen up is negative
                    (axis(Axis::LeftStickX), -axis(Axis::LeftStickY))
                };
                match ev.event {
                    EventType::Connected => log::info!("pad connected: {}", pad.name()),
                    EventType::Disconnected => {
                        log::info!("pad disconnected: {}", pad.name());
                        events.push(None);
                    }
                    EventType::ButtonPressed(b, _) => events.push(Some(PadEvent::Button(b, true))),
                    EventType::ButtonReleased(b, _) => events.push(Some(PadEvent::Button(b, false))),
                    EventType::AxisChanged(Axis::LeftStickX | Axis::LeftStickY, ..) => events.push(Some(PadEvent::Stick(stick()))),
                    _ => {}
                }
            }
            for ev in events {
                match ev {
                    Some(ev) if self.focused => self.pad_event(ev),
                    _ => self.pad.let_go(),
                }
            }
            self.pad_repeat(now);
        }
    }

    #[cfg(feature = "gamepad")]
    fn pad_event(&mut self, ev: PadEvent) {
        use gilrs::Button;
        let p = &mut self.pad;
        match ev {
            PadEvent::Button(b, true) => {
                log::debug!("pad press {b:?}");
                let dir = match b {
                    Button::DPadUp => Some(Dir::N),
                    Button::DPadDown => Some(Dir::S),
                    Button::DPadLeft => Some(Dir::W),
                    Button::DPadRight => Some(Dir::E),
                    _ => None,
                };
                if dir.is_some() {
                    p.dpad = dir;
                    return;
                }
                if let Some(used) = &mut p.back
                    && let Some(act) = layer(b)
                {
                    *used = true;
                    self.acts.push(act);
                    return;
                }
                let act = match b {
                    Button::South => {
                        p.a = Some(false);
                        None
                    }
                    Button::Select => {
                        p.back = Some(false);
                        None
                    }
                    Button::East => Some(Act::Cancel),
                    Button::West => Some(Act::Rotate),
                    Button::North => Some(Act::Operate),
                    Button::LeftTrigger => Some(Act::PrevSlot),
                    Button::RightTrigger => Some(Act::NextSlot),
                    Button::LeftTrigger2 => Some(Act::Undo),
                    Button::RightTrigger2 => Some(Act::Redo),
                    Button::Start => Some(Act::Test),
                    _ => None,
                };
                self.acts.extend(act);
            }
            PadEvent::Button(b, false) => {
                log::debug!("pad release {b:?}");
                match b {
                    Button::DPadUp | Button::DPadDown | Button::DPadLeft | Button::DPadRight => p.dpad = None,
                    Button::South if p.a.take() == Some(false) => self.acts.push(Act::Place),
                    Button::Select if p.back.take() == Some(false) => self.acts.push(Act::Back),
                    _ => {}
                }
            }
            PadEvent::Stick((x, y)) => {
                let dir = if x.hypot(y) < STICK_DEADZONE {
                    None
                } else if x.abs() >= y.abs() {
                    Some(if x < 0.0 { Dir::W } else { Dir::E })
                } else {
                    Some(if y < 0.0 { Dir::N } else { Dir::S })
                };
                if dir != p.stick {
                    log::debug!("pad stick {x:.2},{y:.2} -> {dir:?}");
                    p.stick = dir;
                }
            }
        }
    }

    /// Step the cursor for a held direction: at once, then again after a pause, then quickly.
    #[cfg(feature = "gamepad")]
    fn pad_repeat(&mut self, now: Instant) {
        let p = &mut self.pad;
        let d = match (p.dpad.or(p.stick), p.held) {
            (None, _) => {
                p.held = None;
                return;
            }
            (Some(d), Some((h, next))) if d == h => {
                if now < next {
                    return;
                }
                p.held = Some((d, now + REPEAT_EVERY));
                d
            }
            (Some(d), _) => {
                p.held = Some((d, now + REPEAT_AFTER));
                d
            }
        };
        let act = match &mut p.a {
            Some(laid) => {
                *laid = true;
                Act::Lay(d)
            }
            None => Act::Move(d),
        };
        self.acts.push(act);
    }

    fn key(&mut self, k: KeyEvent) {
        log::debug!("key {:?} {:?} {:?}", k.code, k.kind, k.modifiers);
        if k.kind == KeyEventKind::Release {
            return;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        let dir = match k.code {
            KeyCode::Up => Some(Dir::N),
            KeyCode::Down => Some(Dir::S),
            KeyCode::Left => Some(Dir::W),
            KeyCode::Right => Some(Dir::E),
            _ => None,
        };
        let act = if let Some(d) = dir {
            if shift { Act::Lay(d) } else { Act::Move(d) }
        } else {
            match k.code {
                KeyCode::Char('c' | 'q') if ctrl => Act::Quit,
                KeyCode::Char('z') if ctrl => Act::Undo,
                KeyCode::Char('y') if ctrl => Act::Redo,
                KeyCode::Char(' ') | KeyCode::Enter => Act::Place,
                KeyCode::Char('r') => Act::Rotate,
                KeyCode::Char('x') | KeyCode::Delete | KeyCode::Backspace => Act::Erase,
                KeyCode::Char('e') => Act::Operate,
                KeyCode::Char('t') => Act::Test,
                KeyCode::Char('d') => Act::Drawer,
                KeyCode::Char('p') => Act::Probe,
                KeyCode::Char('n') => Act::Notebook,
                KeyCode::Char('c') => Act::CopperMode,
                KeyCode::Char('f') => Act::Mend,
                KeyCode::Char('u') => Act::Undo,
                KeyCode::Char('m') => Act::Mute,
                KeyCode::F(1) | KeyCode::Char('?') => Act::Help,
                KeyCode::Char('b') => Act::Binder,
                KeyCode::Char('i') => Act::Grow(Grow::Inbox),
                KeyCode::Char('v') => Act::Grow(Grow::Meter),
                KeyCode::Char('o') => Act::Grow(Grow::Scope),
                KeyCode::Char('g') => Act::Grow(Grow::Card),
                KeyCode::Char(c @ '0'..='9') => Act::Slot(c as usize - '0' as usize),
                KeyCode::PageDown => Act::NextJob,
                KeyCode::PageUp => Act::PrevJob,
                KeyCode::Tab => Act::NextSlot,
                KeyCode::BackTab => Act::PrevSlot,
                KeyCode::Esc => Act::Back,
                _ => return,
            }
        };
        self.acts.push(act);
    }

    fn mouse(&mut self, m: MouseEvent) {
        let (x, y) = (m.column, m.row);
        let act = match m.kind {
            MouseEventKind::Down(MouseButton::Middle) => Act::MiddleDown(x, y),
            MouseEventKind::Drag(MouseButton::Middle) => Act::MiddleDrag(x, y),
            MouseEventKind::Down(b) => Act::Down(x, y, b == MouseButton::Right),
            MouseEventKind::Up(_) => Act::Up(x, y),
            MouseEventKind::Drag(b) => Act::Drag(x, y, b == MouseButton::Right),
            MouseEventKind::Moved => Act::Hover(x, y),
            MouseEventKind::ScrollUp => Act::Scroll(x, y, true),
            MouseEventKind::ScrollDown => Act::Scroll(x, y, false),
            _ => return,
        };
        if !matches!(act, Act::Hover(..)) {
            log::debug!("mouse {:?} {x},{y}", m.kind);
        }
        self.acts.push(act);
    }
}

#[cfg(all(test, feature = "gamepad"))]
mod tests {
    use super::*;
    use gilrs::Button;

    fn pad() -> Input {
        Input { acts: Vec::new(), focused: true, pad: Pad::default() }
    }

    fn press(i: &mut Input, b: Button) {
        i.pad_event(PadEvent::Button(b, true));
    }

    fn release(i: &mut Input, b: Button) {
        i.pad_event(PadEvent::Button(b, false));
    }

    #[test]
    fn a_places_on_release_and_lays_copper_while_held() {
        let mut i = pad();
        press(&mut i, Button::South);
        release(&mut i, Button::South);
        assert_eq!(i.acts, [Act::Place]);

        let mut i = pad();
        let t = Instant::now();
        press(&mut i, Button::South);
        press(&mut i, Button::DPadRight);
        i.pad_repeat(t);
        release(&mut i, Button::DPadRight);
        i.pad_repeat(t);
        release(&mut i, Button::South);
        assert_eq!(i.acts, [Act::Lay(Dir::E)], "laying copper doesn't also place");
    }

    #[test]
    fn a_held_direction_repeats() {
        let mut i = pad();
        let t = Instant::now();
        i.pad_event(PadEvent::Stick((0.0, 0.9)));
        i.pad_repeat(t);
        i.pad_repeat(t + Duration::from_millis(100));
        i.pad_repeat(t + REPEAT_AFTER);
        i.pad_repeat(t + REPEAT_AFTER + REPEAT_EVERY);
        assert_eq!(i.acts, [Act::Move(Dir::S); 3]);
    }

    #[test]
    fn back_backs_out_or_is_a_layer() {
        let mut i = pad();
        press(&mut i, Button::Select);
        release(&mut i, Button::Select);
        assert_eq!(i.acts, [Act::Back]);

        let mut i = pad();
        press(&mut i, Button::Select);
        press(&mut i, Button::North);
        press(&mut i, Button::RightTrigger);
        release(&mut i, Button::Select);
        press(&mut i, Button::North);
        assert_eq!(i.acts, [Act::Help, Act::NextJob, Act::Operate], "no Back after the layer was used");
    }

    #[test]
    fn faces_shoulders_triggers_and_start() {
        let mut i = pad();
        for b in [Button::East, Button::West, Button::North, Button::LeftTrigger, Button::RightTrigger, Button::LeftTrigger2, Button::RightTrigger2, Button::Start] {
            press(&mut i, b);
        }
        assert_eq!(i.acts, [Act::Cancel, Act::Rotate, Act::Operate, Act::PrevSlot, Act::NextSlot, Act::Undo, Act::Redo, Act::Test]);
    }
}
