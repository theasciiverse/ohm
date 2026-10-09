//! Raw terminal events into `Act`s. Every raw event is logged before it is mapped (the
//! Asciiverse session-logging convention). Mouse and keyboard are equals: everything can be done
//! with either, and a key acts on whatever tile the pointer last moved the cursor to.

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::circuit::Dir;

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
    /// The next or previous job in the inbox.
    NextJob,
    PrevJob,
    Mend,
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

#[derive(Default)]
pub struct Input {
    pub acts: Vec<Act>,
}

impl Input {
    pub fn on_event(&mut self, ev: Event) {
        match ev {
            Event::Key(k) => self.key(k),
            Event::Mouse(m) => self.mouse(m),
            Event::FocusGained | Event::FocusLost => log::info!("focus {ev:?}"),
            _ => {}
        }
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
