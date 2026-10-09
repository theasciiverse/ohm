//! Drawing: the board's tiles, the parts on them, and the desk around the board.
//!
//! A tile is 5x3 characters. Copper is drawn from each trace's mask (heavy box drawing), parts
//! from hand-drawn art per orientation, mirrored for the other two facings. The colours of a
//! powered board come from the running circuit: copper warms with its voltage, LEDs and lamps
//! glow with their current, resistors redden with their heat.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::circuit::{Board, Cell, Colour, Dir, Kind, Live, Pos};
use crate::input::Act;

pub const TILE_W: u16 = 5;
pub const TILE_H: u16 = 3;

type Rgb = (u8, u8, u8);

// The board: green solder mask, copper, tinned leads.
const BOARD: Rgb = (14, 30, 22);
const HOLE: Rgb = (36, 60, 46);
const HOLE_FOCUS: Rgb = (70, 108, 82);
const COPPER_COLD: Rgb = (122, 78, 42);
const COPPER_HOT: Rgb = (255, 196, 92);
const LEAD: Rgb = (168, 170, 162);
const CURSOR: Rgb = (30, 56, 42);
const CURSOR_ERASE: Rgb = (90, 28, 24);
// The desk.
const DESK: Rgb = (10, 12, 11);
const FRAME: Rgb = (60, 110, 80);
const TEXT: Rgb = (190, 205, 190);
const DIM: Rgb = (100, 120, 105);
const AMBER: Rgb = (255, 176, 0);
const GOOD: Rgb = (90, 220, 110);
const BAD: Rgb = (240, 80, 60);
const PAPER: Rgb = (226, 218, 196);
const INK: Rgb = (40, 44, 60);
const DIM_INK: Rgb = (120, 116, 104);
const LCD: Rgb = (18, 20, 14);
/// A tick in the inbox's list, where room is short: a small square, lit when earned. A
/// quadrant leaves a gap on every side, so ticks in rows below each other stay apart.
const TICK: &str = "▖";

/// A tick on paper and in the report: a box, ticked when earned.
fn checkbox(earned: bool) -> &'static str {
    if earned { "[x]" } else { "[ ]" }
}
/// A tick earned, on paper.
const TICK_INK: Rgb = (190, 110, 0);
/// The scope's trace height in rows, at every panel size.
const SCOPE_ROWS: u16 = 6;

fn c(rgb: Rgb) -> Color {
    Color::Rgb(rgb.0, rgb.1, rgb.2)
}

fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t).round() as u8;
    (m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

fn put(buf: &mut Buffer, x: u16, y: u16, sym: &str, fg: Rgb, bg: Rgb) {
    if let Some(cell) = buf.cell_mut((x, y)) {
        // A fresh cell: whatever was drawn here before (a bold title) doesn't leak through.
        cell.modifier = Modifier::empty();
        cell.set_symbol(sym).set_style(Style::new().fg(c(fg)).bg(c(bg)));
    }
}

fn text(buf: &mut Buffer, x: u16, y: u16, s: &str, fg: Rgb, bg: Rgb) {
    for (i, ch) in s.chars().enumerate() {
        put(buf, x + i as u16, y, ch.encode_utf8(&mut [0; 4]), fg, bg);
    }
}

fn bold(buf: &mut Buffer, x: u16, y: u16, s: &str, fg: Rgb, bg: Rgb) {
    text(buf, x, y, s, fg, bg);
    for i in 0..s.chars().count() {
        if let Some(cell) = buf.cell_mut((x + i as u16, y)) {
            cell.modifier.insert(Modifier::BOLD);
        }
    }
}

fn fill(buf: &mut Buffer, r: Rect, bg: Rgb) {
    for y in r.y..r.y + r.height {
        for x in r.x..r.x + r.width {
            put(buf, x, y, " ", TEXT, bg);
        }
    }
}

/// A rounded frame with a title in its top border and optional status text in the bottom one.
fn frame(buf: &mut Buffer, r: Rect, title: &str, status: &str, bg: Rgb) {
    fill(buf, r, bg);
    let (x1, y1) = (r.x + r.width - 1, r.y + r.height - 1);
    for x in r.x..=x1 {
        put(buf, x, r.y, "─", FRAME, bg);
        put(buf, x, y1, "─", FRAME, bg);
    }
    for y in r.y..=y1 {
        put(buf, r.x, y, "│", FRAME, bg);
        put(buf, x1, y, "│", FRAME, bg);
    }
    put(buf, r.x, r.y, "╭", FRAME, bg);
    put(buf, x1, r.y, "╮", FRAME, bg);
    put(buf, r.x, y1, "╰", FRAME, bg);
    put(buf, x1, y1, "╯", FRAME, bg);
    if !title.is_empty() {
        bold(buf, r.x + 2, r.y, &format!(" {title} "), AMBER, bg);
    }
    if !status.is_empty() {
        text(buf, r.x + 2, y1, &format!(" {status} "), DIM, bg);
    }
}

// ---------------------------------------------------------------- copper

/// Heavy box drawing for copper, from a mask (N=1, E=2, S=4, W=8).
fn copper_glyph(mask: u8) -> &'static str {
    const GLYPHS: [&str; 16] =
        ["·", "╹", "╺", "┗", "╻", "┃", "┏", "┣", "╸", "┛", "━", "┻", "┓", "┫", "┳", "╋"];
    GLYPHS[mask as usize & 15]
}

/// Copper across a tile: arms out to each side in `mask`, the joint in the middle.
fn draw_copper(buf: &mut Buffer, ox: u16, oy: u16, mask: u8, col: Rgb, bg: Rgb) {
    if mask & Dir::W.bit() != 0 {
        put(buf, ox, oy + 1, "━", col, bg);
        put(buf, ox + 1, oy + 1, "━", col, bg);
    }
    if mask & Dir::E.bit() != 0 {
        put(buf, ox + 3, oy + 1, "━", col, bg);
        put(buf, ox + 4, oy + 1, "━", col, bg);
    }
    if mask & Dir::N.bit() != 0 {
        put(buf, ox + 2, oy, "┃", col, bg);
    }
    if mask & Dir::S.bit() != 0 {
        put(buf, ox + 2, oy + 2, "┃", col, bg);
    }
    put(buf, ox + 2, oy + 1, copper_glyph(mask), col, bg);
}

/// A jumper: copper east to west, a tinned wire link hopping over it north to south.
fn draw_jumper(buf: &mut Buffer, ox: u16, oy: u16, ew: Rgb, ns: Rgb, bg: Rgb) {
    put(buf, ox, oy + 1, "━", ew, bg);
    put(buf, ox + 1, oy + 1, "╸", ew, bg);
    put(buf, ox + 3, oy + 1, "╺", ew, bg);
    put(buf, ox + 4, oy + 1, "━", ew, bg);
    for dy in 0..3 {
        put(buf, ox + 2, oy + dy, "┃", ns, bg);
    }
}

// ---------------------------------------------------------------- parts

/// A part's look: three rows of five glyphs, with a colour letter per glyph for the foreground
/// and one for the background (space: the board).
struct Art {
    g: [&'static str; 3],
    f: [&'static str; 3],
    b: [&'static str; 3],
}

// Pin `a` west (H) or north (V). Letters: L lead, T resistor body, 1 2 3 bands, C LED colour,
// G glow, A lamp glass, F filament, P copper cap, B battery body, S silver, R red, K black,
// W white, O ceramic, E electrolytic sleeve, M its stripe.
const RESISTOR_H: Art = Art {
    g: ["     ", "─▐▐▌─", "     "],
    f: ["     ", "L123L", "     "],
    b: ["     ", " TTT ", "     "],
};
const RESISTOR_V: Art = Art {
    g: [" ▗▄▖ ", " ▐▀▌ ", " ▝▀▘ "],
    f: [" TTT ", " T1T ", " T3T "],
    b: ["  L  ", "  2  ", "  L  "],
};
const LED_H: Art = Art {
    g: [" ▗▄▄ ", "─▐██─", " ▝▀▀ "],
    f: [" CCC ", "LCCCL", " CCC "],
    b: ["GGGGG", "G   G", "GGGGG"],
};
const LED_V: Art = Art {
    g: [" ▗▄▖ ", " ▐█▌ ", " ▀▀▀ "],
    f: [" CCC ", " CCC ", " CCC "],
    b: ["GGLGG", "G   G", "GGGGG"],
};
const LAMP_H: Art = Art {
    g: [" ▗▄▖ ", "─▐~▌─", " ▝▀▘ "],
    f: [" AAA ", "LAFAL", " AAA "],
    b: ["GGGGG", "G A G", "GGGGG"],
};
const LAMP_V: Art = Art {
    g: [" ▗▄▖ ", " ▐~▌ ", " ▝▀▘ "],
    f: [" AAA ", " AFA ", " AAA "],
    b: ["GGLGG", "G A G", "GGLGG"],
};
const BATTERY_H: Art = Art {
    g: ["+    ", "▐███▌", "     "],
    f: ["R    ", "SPBBB", "     "],
    b: ["     ", "     ", "     "],
};
const BATTERY_V: Art = Art {
    g: ["  ▄ +", " ▐█▌ ", " ▐█▌ "],
    f: ["  S R", " PPP ", " BBB "],
    b: ["     ", "     ", "     "],
};
const SWITCH_OPEN_H: Art = Art {
    g: ["     ", "─█  ─", "     "],
    f: ["     ", "LW  L", "     "],
    b: ["     ", " KKK ", "     "],
};
const SWITCH_CLOSED_H: Art = Art {
    g: ["     ", "─  █─", "     "],
    f: ["     ", "L  WL", "     "],
    b: ["     ", " KKK ", "     "],
};
const SWITCH_OPEN_V: Art = Art {
    g: ["  █  ", "  █  ", "  █  "],
    f: ["  W  ", "  K  ", "  K  "],
    b: ["     ", "     ", "     "],
};
const SWITCH_CLOSED_V: Art = Art {
    g: ["  █  ", "  █  ", "  █  "],
    f: ["  K  ", "  K  ", "  W  "],
    b: ["     ", "     ", "     "],
};
const BUTTON: Art = Art {
    g: [" ▄▄▄ ", "─█▀█─", " ▀▀▀ "],
    f: [" KKK ", "LKCKL", " KKK "],
    b: ["     ", "  C  ", "     "],
};
const CERAMIC_H: Art = Art {
    g: ["     ", "─▐█▌─", "     "],
    f: ["     ", "LOOOL", "     "],
    b: ["     ", "     ", "     "],
};
const CERAMIC_V: Art = Art {
    g: ["  ▄  ", "  █  ", "  ▀  "],
    f: ["  O  ", "  O  ", "  O  "],
    b: ["  L  ", "     ", "  L  "],
};
const ELECTROLYTIC: Art = Art {
    g: [" ▗▄▖ ", "─▐+▌─", " ▝▀▘ "],
    f: [" EEE ", "LESML", " EEE "],
    b: ["     ", "  E  ", "     "],
};
const TESTPOINT_H: Art = Art {
    g: ["     ", "──O  ", "     "],
    f: ["     ", "LLR  ", "     "],
    b: ["     ", "     ", "     "],
};
const TESTPOINT_V: Art = Art {
    g: ["  │  ", "  O  ", "     "],
    f: ["  L  ", "  R  ", "     "],
    b: ["     ", "     ", "     "],
};
const SUPPLY: Art = Art {
    g: [" ▄▄▄ ", "─+ -─", " ▀▀▀ "],
    f: [" KKK ", "LR WL", " KKK "],
    b: ["     ", " KKK ", "     "],
};

fn mirror_h(g: char) -> char {
    match g {
        '▐' => '▌',
        '▌' => '▐',
        '▗' => '▖',
        '▖' => '▗',
        '▝' => '▘',
        '▘' => '▝',
        '╸' => '╺',
        '╺' => '╸',
        '(' => ')',
        ')' => '(',
        _ => g,
    }
}

fn mirror_v(g: char) -> char {
    match g {
        '▀' => '▄',
        '▄' => '▀',
        '▗' => '▝',
        '▝' => '▗',
        '▖' => '▘',
        '▘' => '▖',
        '╹' => '╻',
        '╻' => '╹',
        _ => g,
    }
}

/// How a part looks right now.
#[derive(Clone, Copy, Debug, Default)]
pub struct Look {
    /// 0 dark to 1 full brightness (LEDs, lamps).
    pub glow: f64,
    /// Load against rating, as `solver::Circuit::heat`.
    pub heat: f64,
    pub burnt: bool,
}

/// The colour code (IEC 60062): each digit's band, then gold, which as a third band divides by
/// ten and at the end of a resistor means within 5 %.
pub const BANDS: [(&str, Rgb); 11] = [
    ("black", (20, 20, 20)),
    ("brown", (120, 66, 30)),
    ("red", (210, 40, 30)),
    ("orange", (240, 130, 20)),
    ("yellow", (240, 210, 30)),
    ("green", (40, 170, 60)),
    ("blue", (40, 90, 220)),
    ("violet", (150, 70, 200)),
    ("grey", (128, 128, 128)),
    ("white", (240, 240, 240)),
    ("gold", (212, 175, 55)),
];
const GOLD: usize = 10;
/// A resistor's body, behind its bands.
const RESISTOR_BODY: Rgb = (214, 186, 140);

/// A value's three bands as indices into `BANDS`: two digits and the multiplier.
fn band_digits(ohms: f64) -> [usize; 3] {
    let exp = ohms.log10().floor() as i32 - 1;
    let sig = (ohms / 10f64.powi(exp)).round() as usize;
    let mult = if exp >= 0 { exp as usize % 10 } else { GOLD };
    [(sig / 10) % 10, sig % 10, mult]
}

fn resistor_bands(ohms: f64) -> [Rgb; 3] {
    band_digits(ohms).map(|d| BANDS[d].1)
}

/// A value's bands by name: "red red brown".
pub fn band_names(ohms: f64) -> String {
    band_digits(ohms).map(|d| BANDS[d].0).join(" ")
}

fn led_rgb(colour: Colour) -> Rgb {
    match colour {
        Colour::Red => (255, 40, 30),
        Colour::Yellow => (255, 200, 0),
        Colour::Green => (40, 230, 70),
        Colour::Blue => (50, 120, 255),
    }
}

/// Draws a part on the tile at `ox, oy`.
fn draw_part(buf: &mut Buffer, ox: u16, oy: u16, kind: Kind, facing: Dir, look: Look, bg: Rgb) {
    let horizontal = matches!(facing, Dir::W | Dir::E);
    let art = match (kind, horizontal) {
        (Kind::Resistor { .. }, true) => &RESISTOR_H,
        (Kind::Resistor { .. }, false) => &RESISTOR_V,
        (Kind::Lamp { .. }, true) => &LAMP_H,
        (Kind::Lamp { .. }, false) => &LAMP_V,
        (Kind::Led(_), true) => &LED_H,
        (Kind::Led(_), false) => &LED_V,
        (Kind::Battery { .. }, true) => &BATTERY_H,
        (Kind::Battery { .. }, false) => &BATTERY_V,
        (Kind::Switch { closed: false }, true) => &SWITCH_OPEN_H,
        (Kind::Switch { closed: true }, true) => &SWITCH_CLOSED_H,
        (Kind::Switch { closed: false }, false) => &SWITCH_OPEN_V,
        (Kind::Switch { closed: true }, false) => &SWITCH_CLOSED_V,
        (Kind::Button { .. }, _) => &BUTTON,
        (Kind::Capacitor { farads }, true) if farads < 1e-6 => &CERAMIC_H,
        (Kind::Capacitor { farads }, false) if farads < 1e-6 => &CERAMIC_V,
        (Kind::Capacitor { .. }, _) => &ELECTROLYTIC,
        (Kind::TestPoint, true) => &TESTPOINT_H,
        (Kind::TestPoint, false) => &TESTPOINT_V,
        (Kind::Supply { .. }, _) => &SUPPLY,
    };
    let bands = match kind {
        Kind::Resistor { ohms, .. } => resistor_bands(ohms),
        _ => [(0, 0, 0); 3],
    };
    let charred: Rgb = (46, 36, 30);
    let warm = |base: Rgb| {
        if look.burnt {
            charred
        } else if look.heat > 1.0 {
            mix(base, (200, 60, 30), (look.heat - 1.0) / 1.0)
        } else {
            base
        }
    };
    let (part_rgb, glow_rgb) = match kind {
        Kind::Led(col) => {
            let full = led_rgb(col);
            let dark = mix(full, (40, 40, 40), 0.6);
            (if look.burnt { charred } else { mix(dark, full, look.glow) }, mix(bg, full, 0.35 * look.glow))
        }
        Kind::Lamp { .. } => (mix((120, 130, 140), (255, 230, 150), look.glow), mix(bg, (255, 220, 120), 0.3 * look.glow)),
        Kind::Button { held } => (if held { (120, 20, 16) } else { (220, 40, 30) }, bg),
        _ => ((0, 0, 0), bg),
    };
    let colour = |letter: char| -> Option<Rgb> {
        Some(match letter {
            'L' => LEAD,
            'T' => warm(RESISTOR_BODY),
            '1' => warm(bands[0]),
            '2' => warm(bands[1]),
            '3' => warm(bands[2]),
            'C' => part_rgb,
            'G' => glow_rgb,
            'A' => part_rgb,
            'F' => mix((150, 110, 70), (255, 255, 240), look.glow),
            'P' => (196, 120, 50),
            'B' => (34, 34, 38),
            'S' => (200, 200, 196),
            'R' => (220, 50, 40),
            'K' => (26, 26, 28),
            'W' => (230, 230, 225),
            'O' => (210, 120, 40),
            'E' => (40, 70, 150),
            'M' => (190, 200, 220),
            _ => return None,
        })
    };
    let flip_h = facing == Dir::E;
    let flip_v = facing == Dir::S;
    for row in 0..3 {
        let src_row = if flip_v { 2 - row } else { row };
        let g: Vec<char> = art.g[src_row].chars().collect();
        let f: Vec<char> = art.f[src_row].chars().collect();
        let b: Vec<char> = art.b[src_row].chars().collect();
        for col in 0..5 {
            let src = if flip_h { 4 - col } else { col };
            let mut ch = g[src];
            if flip_h {
                ch = mirror_h(ch);
            }
            if flip_v {
                ch = mirror_v(ch);
            }
            let fg = colour(f[src]).unwrap_or(TEXT);
            let bgc = colour(b[src]).unwrap_or(bg);
            put(buf, ox + col as u16, oy + row as u16, ch.encode_utf8(&mut [0; 4]), fg, bgc);
        }
    }
    // Smoke from anything running hot: puffs above the part, thickening with the heat.
    if !look.burnt && look.heat > 1.5 {
        let puff = mix(bg, (150, 150, 150), (look.heat - 1.5) * 2.0);
        put(buf, ox + 1, oy, "░", puff, bg);
        put(buf, ox + 3, oy, "▒", puff, bg);
    }
}

// ---------------------------------------------------------------- the board

/// Everything the board needs to be drawn: the board, its running circuit if powered, the
/// current in the copper, the cursor, the picked part shown where it would go, and the clock.
pub struct BoardView<'a> {
    pub board: &'a Board,
    pub live: Option<&'a Live>,
    /// Amps leaving each tile through each side, from `Live::copper_currents`.
    pub flow: Option<&'a [[f64; 4]]>,
    pub cursor: Option<Pos>,
    pub ghost: Option<(Kind, Dir)>,
    /// Where the scope's probe is pinned.
    pub probe: Option<Pos>,
    /// The eraser is in hand: the cursor's tile shows red.
    pub eraser: bool,
    /// The job's own area, in tiles (corner, width, height): its holes show a little brighter.
    pub focus: Option<(Pos, usize, usize)>,
    pub time: f64,
}

impl BoardView<'_> {
    /// The highest source voltage, for scaling copper colour.
    fn vmax(&self) -> f64 {
        self.board
            .parts
            .iter()
            .filter_map(|p| match p.kind {
                Kind::Battery { volts, .. } | Kind::Supply { volts, .. } => Some(volts),
                _ => None,
            })
            .fold(1.0, f64::max)
    }

    fn look(&self, i: usize) -> Look {
        let part = &self.board.parts[i];
        let Some(live) = self.live else { return Look { burnt: part.burnt, ..Look::default() } };
        let r = live.reading(self.board, i);
        let glow = match part.kind {
            Kind::Led(_) => (r.amps / 0.020).clamp(0.0, 1.0).sqrt(),
            Kind::Lamp { watts, .. } => (r.watts / watts).clamp(0.0, 1.0).sqrt(),
            _ => 0.0,
        };
        Look { glow, heat: r.heat, burnt: r.burnt || part.burnt }
    }

    /// Copper colour on a net: cold at 0 V, hot at the highest source voltage.
    fn copper(&self, net: Option<usize>) -> Rgb {
        match (self.live, net) {
            (Some(live), Some(n)) => mix(COPPER_COLD, COPPER_HOT, live.net_volts(n) / self.vmax()),
            _ => mix(COPPER_COLD, COPPER_HOT, 0.25),
        }
    }

    /// Draws the part of the board a layout shows: from its view corner, clipped to the panel.
    /// Tiles are always 5x3 characters: a bigger panel shows more of the board, never bigger
    /// tiles (David, 2026-10-08).
    pub fn draw_view(&self, buf: &mut Buffer, l: &Layout) {
        let (vx, vy) = l.view;
        let cols = (l.inner.width as usize).div_ceil(TILE_W as usize).min(self.board.w - vx);
        let rows = (l.inner.height as usize).div_ceil(TILE_H as usize).min(self.board.h - vy);
        let mut scratch = Buffer::empty(Rect::new(0, 0, cols as u16 * TILE_W, rows as u16 * TILE_H));
        self.draw_tiles(&mut scratch, 0, 0, (vx, vy), (cols, rows));
        let (ox, oy) = l.origin;
        for y in 0..scratch.area.height {
            for x in 0..scratch.area.width {
                let (sx, sy) = (ox + x, oy + y);
                if sx < l.inner.x + l.inner.width && sy < l.inner.y + l.inner.height {
                    buf[(sx, sy)] = scratch[(x, y)].clone();
                }
            }
        }
    }

    pub fn draw_tiles(&self, buf: &mut Buffer, ox: u16, oy: u16, from: (usize, usize), size: (usize, usize)) {
        let b = self.board;
        let in_focus = |p: Pos| self.focus.is_none_or(|(c, w, h)| p.x >= c.x && p.x < c.x + w && p.y >= c.y && p.y < c.y + h);
        for y in from.1..from.1 + size.1 {
            for x in from.0..from.0 + size.0 {
                let p = Pos { x, y };
                let (tx, ty) = (ox + (x - from.0) as u16 * TILE_W, oy + (y - from.1) as u16 * TILE_H);
                let bg = match (self.cursor == Some(p), self.eraser) {
                    (true, true) => CURSOR_ERASE,
                    (true, false) => CURSOR,
                    _ => BOARD,
                };
                fill(buf, Rect::new(tx, ty, TILE_W, TILE_H), bg);
                let net = |d: Dir| self.live.and_then(|l| l.net.net_at(b, p, d));
                match b.cell(p) {
                    Cell::Empty => {
                        put(buf, tx + 2, ty + 1, "·", if in_focus(p) { HOLE_FOCUS } else { HOLE }, bg);
                        if self.cursor == Some(p)
                            && let Some((kind, facing)) = self.ghost
                        {
                            draw_part(buf, tx, ty, kind, facing, Look::default(), bg);
                            fade(buf, Rect::new(tx, ty, TILE_W, TILE_H), bg, 0.55);
                        }
                    }
                    Cell::Blocked => fill(buf, Rect::new(tx, ty, TILE_W, TILE_H), DESK),
                    Cell::Trace(m) => {
                        if self.cursor == Some(p)
                            && let Some((kind, facing)) = self.ghost
                        {
                            // The part in hand, shown where it would replace this copper.
                            draw_part(buf, tx, ty, kind, facing, Look::default(), bg);
                            fade(buf, Rect::new(tx, ty, TILE_W, TILE_H), bg, 0.45);
                        } else {
                            let first = Dir::ALL.into_iter().find(|d| m & d.bit() != 0).unwrap_or(Dir::N);
                            draw_copper(buf, tx, ty, m, self.copper(net(first)), bg);
                            self.ripple(buf, tx, ty, p, &copper_cells(m));
                        }
                    }
                    Cell::Jumper => {
                        draw_jumper(buf, tx, ty, self.copper(net(Dir::E)), LEAD, bg);
                        self.ripple(buf, tx, ty, p, &[(0, 1), (1, 1), (3, 1), (4, 1)]);
                    }
                    Cell::Part(i) => {
                        let part = &b.parts[i];
                        draw_part(buf, tx, ty, part.kind, part.facing, self.look(i), bg);
                    }
                }
                if self.probe == Some(p) {
                    // The scope's probe: a red hook clipped to the tile's corner.
                    put(buf, tx, ty, "P", (255, 70, 60), bg);
                    if let Some(cell) = buf.cell_mut((tx, ty)) {
                        cell.modifier.insert(Modifier::BOLD);
                    }
                }
            }
        }
    }

    /// Current as brightness running through the copper: a ripple travelling the way the
    /// current flows, faster and brighter the more of it there is. `cells` are the copper's
    /// characters, relative to the tile.
    fn ripple(&self, buf: &mut Buffer, tx: u16, ty: u16, p: Pos, cells: &[(u16, u16)]) {
        let Some(flow) = self.flow else { return };
        let out = flow[p.y * self.board.w + p.x];
        let arm = |side: Dir| {
            let f = out[side as usize];
            let toward_plus = matches!(side, Dir::E | Dir::S);
            (f.abs(), if (f > 0.0) == toward_plus { 1.0 } else { -1.0 }, matches!(side, Dir::E | Dir::W))
        };
        let strongest = Dir::ALL.into_iter().map(arm).fold((0.0, 1.0, true), |a, b| if b.0 > a.0 { b } else { a });
        for &(dx, dy) in cells {
            let (amps, sign, horizontal) = match (dx, dy) {
                (0 | 1, 1) => arm(Dir::W),
                (3 | 4, 1) => arm(Dir::E),
                (2, 0) => arm(Dir::N),
                (2, 2) => arm(Dir::S),
                _ => strongest,
            };
            self.crest(buf, tx + dx, ty + dy, amps, sign, horizontal);
        }
    }

    fn crest(&self, buf: &mut Buffer, x: u16, y: u16, amps: f64, sign: f64, horizontal: bool) {
        crest(buf, x, y, amps, sign, horizontal, self.time);
    }
}

/// Brightens one copper character as a ripple of current passes: travelling with `sign` along
/// the row (or column), faster and brighter for more current.
fn crest(buf: &mut Buffer, x: u16, y: u16, amps: f64, sign: f64, horizontal: bool, time: f64) {
    if amps < 1e-4 {
        return;
    }
    // 0.1 mA barely moves, 100 mA races; brightness follows the same scale.
    let level = ((amps / 1e-4).log10() / 3.0).clamp(0.0, 1.0);
    let speed = 3.0 + 9.0 * level;
    let coord = if horizontal { x as f64 } else { y as f64 * 2.0 };
    let wave = ((coord - sign * speed * time) / 6.0 * std::f64::consts::TAU).cos() * 0.5 + 0.5;
    let lift = wave.powi(3) * (0.35 + 0.5 * level);
    if let Some(cell) = buf.cell_mut((x, y))
        && let Color::Rgb(r, g, b) = cell.fg
    {
        cell.set_fg(c(mix((r, g, b), (255, 246, 214), lift)));
    }
}

/// The characters copper covers in a tile, for the ripple.
fn copper_cells(mask: u8) -> Vec<(u16, u16)> {
    let mut cells = vec![(2, 1)];
    if mask & Dir::W.bit() != 0 {
        cells.extend([(0, 1), (1, 1)]);
    }
    if mask & Dir::E.bit() != 0 {
        cells.extend([(3, 1), (4, 1)]);
    }
    if mask & Dir::N.bit() != 0 {
        cells.push((2, 0));
    }
    if mask & Dir::S.bit() != 0 {
        cells.push((2, 2));
    }
    cells
}

/// Fades a rectangle's foreground toward `bg`.
fn fade(buf: &mut Buffer, r: Rect, bg: Rgb, t: f64) {
    for y in r.y..r.y + r.height {
        for x in r.x..r.x + r.width {
            if let Some(cell) = buf.cell_mut((x, y)) {
                if let Color::Rgb(a, b, cc) = cell.fg {
                    cell.set_fg(c(mix((a, b, cc), bg, t)));
                }
                if let Color::Rgb(a, b, cc) = cell.bg {
                    cell.set_bg(c(mix((a, b, cc), bg, t)));
                }
            }
        }
    }
}

// ---------------------------------------------------------------- desk pieces

/// Digits for the meter, 3 pixels wide and 5 tall, drawn with half blocks.
const DIGITS: [[&str; 5]; 10] = [
    ["###", "#.#", "#.#", "#.#", "###"],
    [".#.", "##.", ".#.", ".#.", "###"],
    ["###", "..#", "###", "#..", "###"],
    ["###", "..#", "###", "..#", "###"],
    ["#.#", "#.#", "###", "..#", "..#"],
    ["###", "#..", "###", "..#", "###"],
    ["###", "#..", "###", "#.#", "###"],
    ["###", "..#", "..#", "..#", "..#"],
    ["###", "#.#", "###", "#.#", "###"],
    ["###", "#.#", "###", "..#", "###"],
];

/// Big meter digits: each digit 3 columns, a point 1 column, a minus 3, all 3 rows tall.
fn big_number(buf: &mut Buffer, x: u16, y: u16, s: &str, fg: Rgb, bg: Rgb) -> u16 {
    let mut cx = x;
    for ch in s.chars() {
        let rows: [&str; 5] = match ch {
            '0'..='9' => DIGITS[ch as usize - '0' as usize],
            '.' => ["", "", "", "", "#"],
            '-' => ["...", "...", "###", "...", "..."],
            _ => ["...", "...", "...", "...", "..."],
        };
        let w = if ch == '.' { 1 } else { 3 };
        for col in 0..w {
            for row in 0..3 {
                let px = |r: usize| rows.get(r).and_then(|s| s.chars().nth(col)) == Some('#');
                let (top, bot) = (px(row * 2), px(row * 2 + 1));
                let g = match (top, bot) {
                    (true, true) => "█",
                    (true, false) => "▀",
                    (false, true) => "▄",
                    _ => " ",
                };
                put(buf, cx + col as u16, y + row as u16, g, fg, bg);
            }
        }
        cx += w as u16 + 1;
    }
    cx - x
}

/// A trace over time in half-block pixels: two vertical pixels per cell, one sample per column.
fn scope(buf: &mut Buffer, r: Rect, samples: &[f64]) {
    let trace: Rgb = (90, 255, 140);
    let grid: Rgb = (28, 52, 36);
    for y in r.y..r.y + r.height {
        for x in r.x..r.x + r.width {
            let g = if (x - r.x) % 10 == 0 || (y - r.y) % 2 == 0 && (x - r.x) % 2 == 0 { "·" } else { " " };
            put(buf, x, y, g, grid, LCD);
        }
    }
    let hi = samples.iter().fold(0.0f64, |m, v| m.max(v.abs())).max(1e-6) * 1.15;
    let lo = samples.iter().fold(0.0f64, |m, &v| m.min(v)).min(0.0) - hi * 0.05;
    let px_h = r.height as f64 * 2.0;
    let mut last: Option<i64> = None;
    let start = samples.len().saturating_sub(r.width as usize);
    for (col, &v) in samples[start..].iter().enumerate() {
        let col = col as u16 + r.width.saturating_sub((samples.len() - start) as u16);
        let py = ((1.0 - (v - lo) / (hi - lo)) * (px_h - 1.0)).round().clamp(0.0, px_h - 1.0) as i64;
        let (a, b) = match last {
            Some(l) => (l.min(py), l.max(py)),
            None => (py, py),
        };
        for p in a..=b {
            let cell_y = r.y + (p / 2) as u16;
            let top = p % 2 == 0;
            let existing = buf.cell((r.x + col, cell_y)).map(|c| c.symbol().to_string()).unwrap_or_default();
            let g = match (existing.as_str(), top) {
                ("▄", true) | ("▀", false) | ("█", _) => "█",
                (_, true) => "▀",
                (_, false) => "▄",
            };
            put(buf, r.x + col, cell_y, g, trace, LCD);
        }
        last = Some(py);
    }
}

/// Words wrapped to `width`.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Where everything on the desk is, for drawing and for the mouse.
pub struct Layout {
    pub inbox: Rect,
    /// The board's frame: the middle of the desk, however big it is.
    pub board: Rect,
    /// Inside the frame: where the board shows.
    pub inner: Rect,
    /// The first tile shown (top left), and where on screen it is drawn.
    pub view: (usize, usize),
    pub origin: (u16, u16),
    pub drawers: Rect,
    pub meter: Rect,
    pub scope: Rect,
    pub card: Rect,
    /// Clickable labels in the board's frame.
    pub buttons: Vec<(Rect, Act)>,
    /// One per drawer slot (0: copper).
    pub slots: Vec<Rect>,
    /// What an overlay covers: the board and the drawers.
    pub overlay: Rect,
}

/// The smallest terminal the desk fits: 100x30, the catalog's recording size.
pub const MIN_W: u16 = 100;
pub const MIN_H: u16 = 30;
/// The side columns' narrowest and widest, and the bottom row's lowest, in characters.
const SIDE_MIN: u16 = 22;
const SIDE_AUTO_MAX: u16 = 34;
const BOTTOM_MIN: u16 = 7;

/// Where the panel edges sit, when the player has dragged them; `None` is automatic.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Splits {
    /// Width of the left column (inbox, multimeter).
    pub left: Option<u16>,
    /// Width of the right column (drawers, datasheet).
    pub right: Option<u16>,
    /// Height of the bottom row (multimeter, scope, datasheet).
    pub bottom: Option<u16>,
}

/// A panel edge the mouse can drag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Bottom,
}

pub const BUTTONS: [(&str, Act); 6] = [
    ("TEST", Act::Test),
    ("ERASE", Act::Eraser),
    ("UNDO", Act::Undo),
    ("ROTATE", Act::Rotate),
    ("MEND", Act::Mend),
    ("PROBE", Act::Probe),
];

/// The tiles the board panel must at least show: the size of the first job's board.
const VIEW_MIN: (u16, u16) = (10, 6);

/// The desk laid out over all of `area`, or nothing if it doesn't fit. The board panel is a
/// window onto the workbench, showing as many tiles as fit around `centre`; the panels around it
/// take the rest, as the player has dragged them or, left alone, side columns up to 34 wide and
/// a bottom row about a third of the height.
pub fn layout(area: Rect, board: &Board, slots: usize, splits: Splits, centre: (usize, usize)) -> Option<Layout> {
    if area.width < MIN_W || area.height < MIN_H {
        return None;
    }
    let (need_w, need_h) = (VIEW_MIN.0 * TILE_W + 2, VIEW_MIN.1 * TILE_H + 2);
    let room_w = area.width.saturating_sub(need_w);
    let auto_side = (room_w / 2).clamp(SIDE_MIN, SIDE_AUTO_MAX);
    let left = splits.left.unwrap_or(auto_side).clamp(SIDE_MIN, room_w.saturating_sub(SIDE_MIN).max(SIDE_MIN));
    let right = splits.right.unwrap_or(auto_side).clamp(SIDE_MIN, room_w.saturating_sub(left).max(SIDE_MIN));
    let room_h = area.height.saturating_sub(2 + need_h);
    let auto_bottom = (area.height / 3).clamp(8, 16).min(room_h).max(BOTTOM_MIN);
    let bottom_h = splits.bottom.unwrap_or(auto_bottom).clamp(BOTTOM_MIN, room_h.max(BOTTOM_MIN));
    let (x0, y0) = (area.x, area.y + 1);
    let bw = area.width - left - right;
    let bh = area.height - 2 - bottom_h;
    let inbox = Rect::new(x0, y0, left, bh);
    let br = Rect::new(x0 + left, y0, bw, bh);
    let drawers = Rect::new(br.x + bw, y0, right, bh);
    let meter = Rect::new(x0, y0 + bh, left, bottom_h);
    let scope = Rect::new(br.x, y0 + bh, bw, bottom_h);
    let card = Rect::new(drawers.x, y0 + bh, right, bottom_h);
    let mut buttons = Vec::new();
    let mut bx = br.x + 2;
    for (label, act) in BUTTONS {
        let w = label.len() as u16 + 2;
        buttons.push((Rect::new(bx, br.y + br.height - 1, w, 1), act));
        bx += w + 1;
    }
    let slots = (0..slots).map(|i| Rect::new(drawers.x + 1, drawers.y + 1 + i as u16 * 2, drawers.width - 2, 2)).collect();
    let overlay = Rect::new(br.x, br.y, drawers.x + drawers.width - br.x, bh);
    let inner = Rect::new(br.x + 1, br.y + 1, bw - 2, bh - 2);
    // Along each axis: if the whole board fits, centre it; if not, show the tiles round `centre`.
    let axis = |space: u16, tile: u16, n: usize, c: usize, at: u16| -> (usize, u16) {
        let full = n as u16 * tile;
        if full <= space {
            (0, at + (space - full) / 2)
        } else {
            let shown = (space / tile) as usize;
            (c.saturating_sub(shown / 2).min(n - shown), at)
        }
    };
    let (vx, ox) = axis(inner.width, TILE_W, board.w, centre.0, inner.x);
    let (vy, oy) = axis(inner.height, TILE_H, board.h, centre.1, inner.y);
    Some(Layout { inbox, board: br, inner, view: (vx, vy), origin: (ox, oy), drawers, meter, scope, card, buttons, slots, overlay })
}

/// Whether the inbox lists its jobs on two lines (the open one, and a way to the others) to
/// leave the customer's note room: when listing every arrived job would leave the paper less
/// than 14 rows.
pub fn inbox_compact(l: &Layout, arrived: usize) -> bool {
    l.inbox.height < arrived as u16 + 2 + 3 + 14
}

/// The Notebook's label in the inbox's bottom border.
pub fn notebook_button(l: &Layout) -> Rect {
    Rect::new(l.inbox.x + 2, l.inbox.y + l.inbox.height - 1, 10, 1)
}

/// The panel edge under a screen position: the seam between two columns, or between the rows.
pub fn edge_at(l: &Layout, x: u16, y: u16) -> Option<Edge> {
    let bottom = l.meter.y + l.meter.height;
    if y < l.board.y || y >= bottom || button_at(l, x, y).is_some() {
        return None;
    }
    if x + 1 == l.board.x || x == l.board.x {
        Some(Edge::Left)
    } else if x + 1 == l.drawers.x || x == l.drawers.x {
        Some(Edge::Right)
    } else if y + 1 == l.meter.y || y == l.meter.y {
        Some(Edge::Bottom)
    } else {
        None
    }
}

/// The board tile under a screen position.
pub fn tile_at(l: &Layout, b: &Board, x: u16, y: u16) -> Option<Pos> {
    let (ox, oy) = l.origin;
    if x < ox || y < oy || !inside(l.inner, x, y) {
        return None;
    }
    let (tx, ty) = (l.view.0 + ((x - ox) / TILE_W) as usize, l.view.1 + ((y - oy) / TILE_H) as usize);
    (tx < b.w && ty < b.h).then_some(Pos { x: tx, y: ty })
}

/// Whole tiles the board panel shows across and down.
pub fn tiles_shown(l: &Layout) -> (usize, usize) {
    ((l.inner.width / TILE_W) as usize, (l.inner.height / TILE_H) as usize)
}

/// Where on screen a tile's top-left character is, if it is in view.
#[cfg(test)]
pub fn screen_of(l: &Layout, p: Pos) -> Option<(u16, u16)> {
    if p.x < l.view.0 || p.y < l.view.1 {
        return None;
    }
    let (x, y) = (l.origin.0 + (p.x - l.view.0) as u16 * TILE_W, l.origin.1 + (p.y - l.view.1) as u16 * TILE_H);
    (inside(l.inner, x, y)).then_some((x, y))
}

fn inside(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

/// A clickable label in the board frame under a screen position.
pub fn button_at(l: &Layout, x: u16, y: u16) -> Option<Act> {
    l.buttons.iter().find(|(r, _)| inside(*r, x, y)).map(|(_, a)| *a)
}

/// The drawer slot under a screen position.
pub fn slot_at(l: &Layout, x: u16, y: u16) -> Option<usize> {
    l.slots.iter().position(|r| inside(*r, x, y))
}

pub fn in_rect(r: Rect, x: u16, y: u16) -> bool {
    inside(r, x, y)
}

/// One row in the drawers: what it is, how many, what it costs.
pub struct Slot {
    pub name: String,
    pub count: String,
    pub price: String,
    /// The part, for its picture; `None` for copper.
    pub kind: Option<Kind>,
}

pub struct Meter {
    pub value: f64,
    pub unit: &'static str,
    pub what: String,
    pub second: String,
}

/// A part's card, as in the binder.
pub struct Card {
    pub name: String,
    pub kind: Kind,
    pub price: String,
    /// Three short lines beside the part's picture.
    pub side: [String; 3],
    /// What it is doing right now.
    pub now: String,
    /// One rule: a keyword and its reminder.
    pub rule: (String, String),
}

/// What the desk shows besides the board.
pub struct DeskView {
    pub title: String,
    pub ledger: String,
    /// The jobs that have arrived: name, open, and its ticks (works, thrifty, and frugal where
    /// the battery counts).
    pub inbox: Vec<(String, bool, Vec<bool>)>,
    /// Jobs still to come, shown as sealed letters.
    pub sealed: usize,
    pub customer: String,
    pub note: Vec<String>,
    pub spec: Vec<(String, Option<bool>)>,
    /// The job's goals beyond working (parts cost, battery life), each earned or not.
    pub goals: Vec<(String, bool)>,
    /// The job has passed: the spec's tick is earned.
    pub works: bool,
    pub slots: Vec<Slot>,
    pub pick: usize,
    pub copper_mode: bool,
    pub meter: Meter,
    pub scope: Vec<f64>,
    pub scope_label: String,
    /// "A" or "V", for the scope's numbers.
    pub scope_unit: &'static str,
    pub card: Option<Card>,
    pub status: String,
    pub hints: String,
    pub message: Option<String>,
    /// A panel edge to light: under the mouse, or being dragged.
    pub edge: Option<Edge>,
}

/// The whole desk.
pub fn desk(buf: &mut Buffer, area: Rect, l: &Layout, board: &BoardView, d: &DeskView) {
    fill(buf, area, DESK);
    let (x0, y0) = (area.x, area.y);

    // Title bar.
    bold(buf, x0 + 1, y0, "OHM'S WORKSHOP", AMBER, DESK);
    text(buf, x0 + 16, y0, &format!("· {}", d.title), TEXT, DESK);
    let ledger = format!("ledger {}", d.ledger);
    text(buf, x0 + area.width - ledger.len() as u16 - 1, y0, &ledger, AMBER, DESK);

    // Inbox: the jobs, then the open job's note and spec on paper; the Notebook in its border.
    let inbox = l.inbox;
    frame(buf, inbox, "INBOX", "", DESK);
    let nb = notebook_button(l);
    bold(buf, nb.x, nb.y, " NOTEBOOK ", DESK, FRAME);
    let rows = if inbox_compact(l, d.inbox.len()) {
        // Short of room: the open job, and a line to step to the others.
        let open = d.inbox.iter().find(|j| j.1).map(|j| j.0.as_str()).unwrap_or("");
        text(buf, inbox.x + 2, inbox.y + 1, &format!("> {open}"), AMBER, DESK);
        let done = d.inbox.iter().filter(|j| j.2[0]).count();
        let line = format!("< {done} done · {} sealed >", d.sealed);
        text(buf, inbox.x + 2, inbox.y + 2, &line, DIM, DESK);
        2
    } else {
        for (i, (job, open, ticks)) in d.inbox.iter().enumerate() {
            let y = inbox.y + 1 + i as u16;
            for (k, t) in ticks.iter().enumerate() {
                put(buf, inbox.x + 2 + k as u16, y, TICK, if *t { AMBER } else { DIM }, DESK);
            }
            let col = if *open { AMBER } else if ticks[0] { GOOD } else { TEXT };
            // The open job is the amber one, on a lit row.
            let bg = if *open { CURSOR } else { DESK };
            text(buf, inbox.x + 6, y, &format!("{job:<w$}", w = inbox.width.saturating_sub(8) as usize), col, bg);
        }
        let mut rows = d.inbox.len() as u16;
        if d.sealed > 0 {
            let s = if d.sealed == 1 { "" } else { "s" };
            text(buf, inbox.x + 6, inbox.y + 1 + rows, &format!("{} sealed letter{s}", d.sealed), DIM, DESK);
            rows += 1;
        }
        rows
    };
    let top = 2 + rows;
    let paper = Rect::new(inbox.x + 1, inbox.y + top, inbox.width - 2, inbox.height.saturating_sub(top + 1));
    fill(buf, paper, PAPER);
    let pw = paper.width as usize - 2;
    let bottom = paper.y + paper.height;
    // The spec sits at the foot of the paper and always shows; the note takes what's left.
    let spec: Vec<(usize, String)> = d.spec.iter().enumerate().flat_map(|(i, (line, _))| wrap(line, paper.width as usize - 3).into_iter().map(move |l| (i, l))).collect();
    let goal_lines: usize = d.goals.iter().map(|(line, _)| wrap(line, paper.width as usize - 5).len()).sum();
    let spec_y = bottom.saturating_sub((spec.len() + goal_lines) as u16 + 1).max(paper.y);
    bold(buf, paper.x + 1, paper.y, &d.customer, INK, PAPER);
    let note: Vec<String> = d.note.iter().flat_map(|p| wrap(p, pw)).collect();
    let note_room = spec_y.saturating_sub(paper.y + 3) as usize;
    for (k, line) in note.iter().take(note_room).enumerate() {
        let cut = note.len() > note_room && k + 1 == note_room;
        let line = if cut { "... (drag to see)".chars().take(pw).collect() } else { line.clone() };
        text(buf, paper.x + 1, paper.y + 2 + k as u16, &line, if cut { DIM_INK } else { INK }, PAPER);
    }
    if spec_y > paper.y {
        text(buf, paper.x + 1, spec_y, checkbox(d.works), if d.works { TICK_INK } else { DIM_INK }, PAPER);
        bold(buf, paper.x + 5, spec_y, "Spec", INK, PAPER);
    }
    let mut last = usize::MAX;
    for (k, (i, line)) in spec.iter().enumerate() {
        let ly = spec_y + 1 + k as u16;
        if ly >= bottom {
            break;
        }
        if *i != last {
            let (mark, col) = match d.spec[*i].1 {
                Some(true) => ("+", (30, 120, 50)),
                Some(false) => ("x", (170, 40, 30)),
                None => ("·", INK),
            };
            text(buf, paper.x + 1, ly, mark, col, PAPER);
            last = *i;
        }
        text(buf, paper.x + 3, ly, line, INK, PAPER);
    }
    // The goals beyond working, under the spec: the master's par to match.
    let goals_y = spec_y + 1 + spec.len() as u16;
    let mut k = 0;
    for (line, t) in &d.goals {
        for (j, part) in wrap(line, paper.width as usize - 5).into_iter().enumerate() {
            let ly = goals_y + k;
            k += 1;
            if ly >= bottom {
                break;
            }
            if j == 0 {
                text(buf, paper.x + 1, ly, checkbox(*t), if *t { TICK_INK } else { DIM_INK }, PAPER);
            }
            text(buf, paper.x + 5, ly, &part, INK, PAPER);
        }
    }

    // The board panel is a window onto one big perfboard: a bigger panel shows more of it,
    // never bigger tiles.
    frame(buf, l.board, "BOARD", "", DESK);
    fill(buf, l.inner, DESK);
    board.draw_view(buf, l);
    for ((r, act), (label, _)) in l.buttons.iter().zip(BUTTONS) {
        let lit = *act == Act::Eraser && board.eraser;
        bold(buf, r.x, r.y, &format!(" {label} "), DESK, if lit { AMBER } else { FRAME });
    }
    let status = format!(" {} ", d.status);
    text(buf, l.board.x + l.board.width - status.chars().count() as u16 - 2, l.board.y, &status, DIM, DESK);

    // Parts drawers.
    let dr = l.drawers;
    frame(buf, dr, "DRAWERS +", "", DESK);
    for (i, (slot, r)) in d.slots.iter().zip(&l.slots).enumerate() {
        let picked = i == d.pick;
        let (fg, bg) = if picked { (DESK, AMBER) } else { (TEXT, DESK) };
        text(buf, r.x, r.y, &format!("{i}"), DIM, DESK);
        let w = r.width as usize - 2;
        text(buf, r.x + 2, r.y, &format!("{:<w$}", slot.name, w = w), fg, bg);
        text(buf, r.x + 2, r.y + 1, &format!("{:<6}{:>w$}", slot.count, slot.price, w = w - 6), DIM, DESK);
    }
    if d.copper_mode {
        text(buf, dr.x + 2, dr.y + dr.height - 1, " laying copper ", AMBER, DESK);
    }

    // Multimeter.
    let mm = l.meter;
    frame(buf, mm, "MULTIMETER", "reads the cursor", DESK);
    let lcd = Rect::new(mm.x + 2, mm.y + 1, mm.width - 4, 5);
    fill(buf, lcd, LCD);
    let m = &d.meter;
    let (scaled, unit) = meter_scale(m.value, m.unit);
    let w = big_number(buf, lcd.x + 1, lcd.y, &scaled, AMBER, LCD);
    bold(buf, lcd.x + 2 + w, lcd.y + 2, &unit, AMBER, LCD);
    text(buf, lcd.x + 1, lcd.y + 3, &m.what, DIM, LCD);
    text(buf, lcd.x + 1, lcd.y + 4, &m.second, TEXT, LCD);

    // Scope.
    frame(buf, l.scope, "SCOPE", &d.scope_label, DESK);
    let sr = l.scope;
    // The trace keeps its height at any size: a bigger scope shows more history across, and
    // below the trace, its numbers.
    let plot_h = SCOPE_ROWS.min(sr.height - 2);
    scope(buf, Rect::new(sr.x + 1, sr.y + 1, sr.width - 2, plot_h), &d.scope);
    let shown = (sr.width as usize - 2).min(d.scope.len());
    let recent = &d.scope[d.scope.len() - shown..];
    if sr.height >= plot_h + 4 && !recent.is_empty() {
        let unit = if d.scope_unit.is_empty() { "A" } else { d.scope_unit };
        let (lo, hi) = recent.iter().fold((f64::MAX, f64::MIN), |(a, b), &v| (a.min(v), b.max(v)));
        let mean = recent.iter().sum::<f64>() / recent.len() as f64;
        let now = recent[recent.len() - 1];
        let line = format!(
            "now {}   min {}   max {}   mean {}   {:.1} s shown",
            crate::bench::si(now, unit),
            crate::bench::si(lo, unit),
            crate::bench::si(hi, unit),
            crate::bench::si(mean, unit),
            shown as f64 / 60.0
        );
        text(buf, sr.x + 2, sr.y + 2 + plot_h, &line.chars().take(sr.width as usize - 4).collect::<String>(), TEXT, DESK);
    }

    // The card of the part under the cursor.
    let cr = l.card;
    frame(buf, cr, "DATASHEET", "", DESK);
    if let Some(card) = &d.card {
        bold(buf, cr.x + 2, cr.y + 1, &card.name, PAPER, DESK);
        text(buf, cr.x + cr.width - card.price.chars().count() as u16 - 2, cr.y + 1, &card.price, AMBER, DESK);
        draw_part(buf, cr.x + 1, cr.y + 2, card.kind, Dir::W, Look::default(), DESK);
        for (i, line) in card.side.iter().enumerate() {
            text(buf, cr.x + 7, cr.y + 2 + i as u16, line, TEXT, DESK);
        }
        text(buf, cr.x + 2, cr.y + 5, &card.now, GOOD, DESK);
        bold(buf, cr.x + 2, cr.y + 6, &card.rule.0, TEXT, DESK);
        text(buf, cr.x + 3 + card.rule.0.chars().count() as u16, cr.y + 6, &card.rule.1, DIM, DESK);
    } else {
        text(buf, cr.x + 2, cr.y + 2, "Point at a part", DIM, DESK);
        text(buf, cr.x + 2, cr.y + 3, "to read its card.", DIM, DESK);
    }

    // A panel edge under the mouse, or being dragged, lights up.
    if let Some(edge) = d.edge {
        let (top, bottom) = (l.board.y, l.meter.y + l.meter.height);
        let cells: Vec<(u16, u16)> = match edge {
            Edge::Left => (top..bottom).flat_map(|y| [(l.board.x - 1, y), (l.board.x, y)]).collect(),
            Edge::Right => (top..bottom).flat_map(|y| [(l.drawers.x - 1, y), (l.drawers.x, y)]).collect(),
            Edge::Bottom => (l.inbox.x..l.card.x + l.card.width).flat_map(|x| [(x, l.meter.y - 1), (x, l.meter.y)]).collect(),
        };
        for (x, y) in cells {
            if let Some(cell) = buf.cell_mut((x, y))
                && matches!(cell.symbol(), "│" | "─" | "╭" | "╮" | "╰" | "╯")
            {
                cell.set_fg(c(AMBER));
            }
        }
    }

    // Hint row, or a message.
    let bottom = y0 + area.height - 1;
    match &d.message {
        Some(msg) => bold(buf, x0 + 1, bottom, msg, AMBER, DESK),
        None => text(buf, x0 + 1, bottom, &d.hints, DIM, DESK),
    }
}

/// A meter reading in three significant figures with an SI prefix.
fn meter_scale(v: f64, unit: &str) -> (String, String) {
    let a = v.abs();
    let (scale, prefix) = if a >= 1.0 || a == 0.0 {
        (1.0, "")
    } else if a >= 1e-3 {
        (1e3, "m")
    } else {
        (1e6, "µ")
    };
    let s = v * scale;
    let digits = if s.abs() >= 100.0 { 0 } else if s.abs() >= 10.0 { 1 } else { 2 };
    (format!("{s:.digits$}"), format!("{prefix}{unit}"))
}

// ---------------------------------------------------------------- overlays

/// Card size in the drawer overlay.
const OCARD_W: u16 = 18;
const OCARD_H: u16 = 6;

/// How many drawer cards fit across and down the overlay.
pub fn overlay_grid(l: &Layout) -> (usize, usize) {
    (((l.overlay.width - 2) / (OCARD_W + 1)) as usize, ((l.overlay.height - 2) / OCARD_H) as usize)
}

fn overlay_card_rect(l: &Layout, i: usize, scroll: usize) -> Option<Rect> {
    let (across, down) = overlay_grid(l);
    let row = i / across;
    if row < scroll || row >= scroll + down {
        return None;
    }
    let x = l.overlay.x + 1 + (i % across) as u16 * (OCARD_W + 1);
    let y = l.overlay.y + 1 + (row - scroll) as u16 * OCARD_H;
    Some(Rect::new(x, y, OCARD_W, OCARD_H))
}

/// The drawer card under a screen position.
pub fn overlay_card_at(l: &Layout, n: usize, scroll: usize, x: u16, y: u16) -> Option<usize> {
    (0..n).find(|&i| overlay_card_rect(l, i, scroll).is_some_and(|r| inside(r, x, y)))
}

/// Every drawer as a card with its picture, over the board and the drawers list.
pub fn drawer_overlay(buf: &mut Buffer, l: &Layout, slots: &[Slot], sel: usize, scroll: usize) {
    let o = l.overlay;
    let (across, down) = overlay_grid(l);
    let rows = slots.len().div_ceil(across);
    let status = if rows > down { format!("{}-{} of {} rows · wheel scrolls", scroll + 1, (scroll + down).min(rows), rows) } else { String::new() };
    frame(buf, o, "DRAWERS -", &status, DESK);
    text(buf, o.x + 14, o.y, " pick a part: click it, or arrows and Enter · Esc goes back ", DIM, DESK);
    for (i, slot) in slots.iter().enumerate() {
        let Some(r) = overlay_card_rect(l, i, scroll) else { continue };
        let picked = i == sel;
        let edge = if picked { AMBER } else { FRAME };
        fill(buf, r, BOARD);
        for x in r.x..r.x + r.width {
            put(buf, x, r.y, "▀", edge, BOARD);
            put(buf, x, r.y + r.height - 1, "▄", edge, BOARD);
        }
        let art_x = r.x + 1;
        match slot.kind {
            Some(kind) => draw_part(buf, art_x, r.y + 1, kind, Dir::W, Look::default(), BOARD),
            None => draw_copper(buf, art_x, r.y + 1, Dir::E.bit() | Dir::W.bit() | Dir::S.bit(), COPPER_HOT, BOARD),
        }
        let tx = r.x + 7;
        // The kind of part on the first line, its value on the second.
        let name_lines: Vec<String> = match slot.name.split_once(' ') {
            Some((a, b)) => vec![a.to_string(), b.to_string()],
            None => vec![slot.name.clone()],
        };
        for (k, line) in name_lines.iter().take(2).enumerate() {
            bold(buf, tx, r.y + 1 + k as u16, line, if picked { AMBER } else { TEXT }, BOARD);
        }
        text(buf, tx, r.y + 3, &slot.count, DIM, BOARD);
        text(buf, tx, r.y + 4, &slot.price, AMBER, BOARD);
        text(buf, r.x + 1, r.y + 4, &format!("{i}"), DIM, BOARD);
    }
}

/// Where the Notebook's parts sit inside the overlay.
pub struct BookLayout {
    /// The page titles, one per row.
    pub list: Rect,
    /// The open page: its text above, its example circuit at the foot.
    pub page: Rect,
}

pub fn book_layout(l: &Layout) -> BookLayout {
    let o = l.overlay;
    let list = Rect::new(o.x + 2, o.y + 2, 24, o.height - 3);
    let page = Rect::new(list.x + list.width + 2, o.y + 2, o.width.saturating_sub(list.width + 6), o.height - 3);
    BookLayout { list, page }
}

/// Where the example circuit's first tile is drawn, if it fits.
pub fn book_example_origin(b: &BookLayout, board: &Board) -> Option<(u16, u16)> {
    let (w, h) = (board.w as u16 * TILE_W, board.h as u16 * TILE_H);
    (w <= b.page.width && h + 3 <= b.page.height).then_some((b.page.x, b.page.y + b.page.height - h))
}

/// The Master's Notebook: the pages down the left (locked ones dim), the open page's text, its
/// sources, and its example circuit running at the foot with each part's reading beside it.
#[allow(clippy::too_many_arguments)]
pub fn book(
    buf: &mut Buffer,
    l: &Layout,
    titles: &[(String, bool)],
    sel: usize,
    page: &crate::book::Page,
    scroll: usize,
    example: Option<(&Board, &Live, &[[f64; 4]], &[String])>,
    time: f64,
) {
    let o = l.overlay;
    frame(buf, o, "THE MASTER'S NOTEBOOK", "arrows or click a page · wheel scrolls · n or Esc closes", DESK);
    let bl = book_layout(l);
    for (i, (title, open)) in titles.iter().enumerate() {
        if i as u16 >= bl.list.height {
            break;
        }
        let (fg, bg) = match (i == sel, open) {
            (true, _) => (DESK, AMBER),
            (false, true) => (TEXT, DESK),
            (false, false) => (DIM, DESK),
        };
        let label = if *open { title.clone() } else { "(not yet)".to_string() };
        text(buf, bl.list.x, bl.list.y + i as u16, &format!("{:<w$}", label, w = bl.list.width as usize), fg, bg);
    }
    // The page itself, on paper.
    let paper = bl.page;
    let origin = example.and_then(|(b, ..)| book_example_origin(&bl, b));
    let text_bottom = origin.map(|(_, y)| y.saturating_sub(1)).unwrap_or(paper.y + paper.height);
    let w = paper.width as usize;
    let lines = page_lines(page, w);
    let mut room = text_bottom.saturating_sub(paper.y) as usize;
    let overflow = lines.len() > room;
    if overflow {
        // The last line says there's more.
        room = room.saturating_sub(1);
    }
    let scroll = scroll.min(lines.len().saturating_sub(room));
    for (k, line) in lines.iter().skip(scroll).take(room).enumerate() {
        let mut x = paper.x;
        for (s, fg, bg, strong) in line {
            if *strong {
                bold(buf, x, paper.y + k as u16, s, *fg, *bg);
            } else {
                text(buf, x, paper.y + k as u16, s, *fg, *bg);
            }
            x += s.chars().count() as u16;
        }
    }
    if overflow {
        let note = if scroll + room < lines.len() { "more below: scroll down" } else { "the end: scroll up for the start" };
        text(buf, paper.x, paper.y + room as u16, note, AMBER, DESK);
    }
    if let (Some((b, live, flow, readings)), Some((ox, oy))) = (example, origin) {
        let view = BoardView { board: b, live: Some(live), flow: Some(flow), cursor: None, ghost: None, probe: None, eraser: false, focus: None, time };
        let mut scratch = Buffer::empty(Rect::new(0, 0, b.w as u16 * TILE_W, b.h as u16 * TILE_H));
        view.draw_tiles(&mut scratch, 0, 0, (0, 0), (b.w, b.h));
        for y in 0..scratch.area.height {
            for x in 0..scratch.area.width {
                buf[(ox + x, oy + y)] = scratch[(x, y)].clone();
            }
        }
        let rx = ox + scratch.area.width + 2;
        bold(buf, rx, oy, "now:", TEXT, DESK);
        for (k, r) in readings.iter().enumerate() {
            text(buf, rx, oy + 1 + k as u16, r, GOOD, DESK);
        }
        if b.parts.iter().any(|p| matches!(p.kind, Kind::Switch { .. } | Kind::Button { .. })) {
            text(buf, rx, oy + 2 + readings.len() as u16, "click a switch", DIM, DESK);
        }
    }
}

/// A run of text in one style: the text, its colours, bold or not.
type Span = (String, Rgb, Rgb, bool);

/// A Notebook page laid out in lines `w` wide: the title, the text with its charts and
/// drawings, the sources.
fn page_lines(page: &crate::book::Page, w: usize) -> Vec<Vec<Span>> {
    use crate::book::Block;
    let plain = |s: String, fg: Rgb| vec![(s, fg, DESK, false)];
    // A band on a scrap of resistor body, so black and white show against the desk.
    let swatch = |colour: Rgb| -> Vec<Span> {
        vec![(" ".into(), TEXT, RESISTOR_BODY, false), ("  ".into(), TEXT, colour, false), (" ".into(), TEXT, RESISTOR_BODY, false)]
    };
    let mut lines = vec![vec![(page.title.clone(), AMBER, DESK, true)], vec![]];
    for block in &page.blocks {
        match block {
            Block::Para(p) => lines.extend(wrap(p, w).into_iter().map(|l| plain(l, TEXT))),
            Block::Colours => {
                // As many colours to a row as fit: a wider page shows the chart in fewer rows.
                const ENTRY: usize = 15;
                let across = (w / ENTRY).clamp(1, 5);
                for row in (0..10).collect::<Vec<_>>().chunks(across) {
                    let mut line = Vec::new();
                    for &d in row {
                        line.extend(swatch(BANDS[d].1));
                        let label = format!(" {d} {}", BANDS[d].0);
                        line.push((format!("{label:<w$}", w = ENTRY - 4), TEXT, DESK, false));
                    }
                    lines.push(line);
                    // A gap, so a swatch doesn't run into the one below it.
                    lines.push(vec![]);
                }
                let mut line = swatch(BANDS[GOLD].1);
                line.push((" gold, set apart at the end: within 5 %".into(), TEXT, DESK, false));
                lines.push(line);
            }
            Block::Resistor(ohms) => {
                // Leads, then the body: three bands crowded to one end, the gold one set apart.
                let mut line: Vec<Span> = vec![("  ───".into(), LEAD, DESK, false)];
                let body = |n: usize| -> Span { (" ".repeat(n), TEXT, RESISTOR_BODY, false) };
                line.push(body(1));
                for d in band_digits(*ohms) {
                    line.push((" ".into(), TEXT, BANDS[d].1, false));
                    line.push(body(1));
                }
                line.push(body(2));
                line.push((" ".into(), TEXT, BANDS[GOLD].1, false));
                line.push(body(1));
                line.push(("───  ".into(), LEAD, DESK, false));
                line.push((format!("{} gold", band_names(*ohms)), DIM, DESK, false));
                lines.push(line);
            }
        }
        lines.push(vec![]);
    }
    for src in &page.sources {
        lines.extend(wrap(&format!("Source: {src}"), w).into_iter().map(|l| plain(l, DIM)));
    }
    lines
}

/// The test bench's report: each case, then the money or the first hint.
pub struct Report {
    pub pass: bool,
    pub rows: Vec<(String, bool, String)>,
    /// Each goal, this build against the master's, earned or not.
    pub goals: Vec<(String, bool)>,
    /// The master's word on a tick missed.
    pub notes: Vec<String>,
    pub summary: Vec<String>,
    pub hint: Option<[String; 3]>,
}

pub fn report(buf: &mut Buffer, l: &Layout, r: &Report) {
    let o = l.overlay;
    frame(buf, o, if r.pass { "TEST BENCH · PASSED" } else { "TEST BENCH" }, "Enter or click to go back", DESK);
    let mut y = o.y + 2;
    let w = (o.width - 6) as usize;
    for (name, pass, why) in &r.rows {
        let (mark, col) = if *pass { ("pass", GOOD) } else { ("FAIL", BAD) };
        bold(buf, o.x + 2, y, mark, col, DESK);
        bold(buf, o.x + 8, y, name, TEXT, DESK);
        y += 1;
        for line in wrap(why, w - 6) {
            text(buf, o.x + 8, y, &line, DIM, DESK);
            y += 1;
        }
    }
    y += 1;
    // The ticks: this build against the old master's.
    if r.pass {
        for (line, t) in &r.goals {
            text(buf, o.x + 2, y, checkbox(*t), if *t { AMBER } else { DIM }, DESK);
            text(buf, o.x + 6, y, line, if *t { TEXT } else { DIM }, DESK);
            y += 1;
        }
        for note in &r.notes {
            for (k, line) in wrap(&format!("The master: {note}"), w - 4).into_iter().enumerate() {
                text(buf, o.x + 6, y, &line, if k == 0 { AMBER } else { TEXT }, DESK);
                y += 1;
            }
        }
        y += 1;
    }
    for line in r.summary.iter().flat_map(|s| wrap(s, w)) {
        if y < o.y + o.height - 1 {
            bold(buf, o.x + 2, y, &line, if r.pass { AMBER } else { TEXT }, DESK);
        }
        y += 1;
    }
    if let Some([nudge, detail, fix]) = &r.hint {
        bold(buf, o.x + 2, y, nudge, AMBER, DESK);
        y += 1;
        for line in wrap(detail, w).into_iter().chain(wrap(fix, w)) {
            if y < o.y + o.height - 1 {
                text(buf, o.x + 2, y, &line, TEXT, DESK);
            }
            y += 1;
        }
    }
}

/// The title in copper: one trace mask per tile, three letters on a strip of board.
const LOGO: [&str; 4] = ["┌─┐ ╷ ╷ ┌┬┐", "│ │ │ │ │││", "│ │ ├─┤ │╵│", "└─┘ ╵ ╵ ╵ ╵"];
/// The logo's board: a tile of margin left and right, one row above.
const LOGO_TILES: (u16, u16) = (13, 5);

fn title_top(area: Rect) -> u16 {
    area.y + area.height.saturating_sub(LOGO_TILES.1 * TILE_H + 12) / 2
}

/// Where the launch page's menu entries are, for drawing and for the mouse.
pub fn title_items(area: Rect, n: usize) -> Vec<Rect> {
    let y = title_top(area) + LOGO_TILES.1 * TILE_H + 4;
    let w = 22;
    (0..n).map(|i| Rect::new(area.x + (area.width.saturating_sub(w)) / 2, y + i as u16, w, 1)).collect()
}

/// The launch page: the name in copper with current running through it, the menu, a line of
/// progress. Fixed size, centred: a bigger terminal shows more desk round it.
pub fn title(buf: &mut Buffer, area: Rect, menu: &[String], sel: usize, line: &str, time: f64) {
    fill(buf, area, DESK);
    let (pw, ph) = (LOGO_TILES.0 * TILE_W, LOGO_TILES.1 * TILE_H);
    let x0 = area.x + area.width.saturating_sub(pw) / 2;
    let y0 = title_top(area);
    fill(buf, Rect::new(x0, y0, pw, ph), BOARD);
    for ty in 0..LOGO_TILES.1 {
        for tx in 0..LOGO_TILES.0 {
            put(buf, x0 + tx * TILE_W + 2, y0 + ty * TILE_H + 1, "·", HOLE, BOARD);
        }
    }
    for (row, line) in LOGO.iter().enumerate() {
        for (col, ch) in line.chars().enumerate() {
            let Some(m) = crate::circuit::trace_mask_of(ch) else { continue };
            let (tx, ty) = (x0 + (col as u16 + 1) * TILE_W, y0 + (row as u16 + 1) * TILE_H);
            fill(buf, Rect::new(tx, ty, TILE_W, TILE_H), BOARD);
            let warm = mix(COPPER_COLD, COPPER_HOT, 0.35 + 0.5 * (1.0 - row as f64 / 4.0));
            draw_copper(buf, tx, ty, m, warm, BOARD);
            // Current runs right along the rows and down the columns.
            for (dx, dy) in copper_cells(m) {
                let horizontal = dy == 1;
                crest(buf, tx + dx, ty + dy, 0.012, 1.0, horizontal, time);
            }
        }
    }
    // A lit LED in the corner of the board.
    draw_part(buf, x0 + 12 * TILE_W, y0 + TILE_H, Kind::Led(Colour::Red), Dir::N, Look { glow: 1.0, ..Look::default() }, BOARD);
    let mut y = y0 + ph + 1;
    let name = "O H M ' S   W O R K S H O P";
    bold(buf, area.x + area.width.saturating_sub(name.len() as u16) / 2, y, name, AMBER, DESK);
    y += 1;
    let tag = "a repair shop for small electronics";
    text(buf, area.x + area.width.saturating_sub(tag.len() as u16) / 2, y, tag, DIM, DESK);
    for (i, (r, label)) in title_items(area, menu.len()).iter().zip(menu).enumerate() {
        let (fg, bg) = if i == sel { (DESK, AMBER) } else { (TEXT, DESK) };
        let padded = format!("{label:^w$}", w = r.width as usize);
        bold(buf, r.x, r.y, &padded, fg, bg);
    }
    let ly = title_items(area, menu.len()).last().map(|r| r.y + 2).unwrap_or(y + 2);
    text(buf, area.x + area.width.saturating_sub(line.chars().count() as u16) / 2, ly, line, DIM, DESK);
    let hint = "arrows and Enter, or click · Ctrl+C quits";
    text(buf, area.x + area.width.saturating_sub(hint.chars().count() as u16) / 2, area.y + area.height - 1, hint, DIM, DESK);
}

/// The desk doesn't fit: say how big it needs to be.
pub fn too_small(buf: &mut Buffer, area: Rect) {
    fill(buf, area, DESK);
    let msg = format!("Ohm's Workshop needs {MIN_W}x{MIN_H}; this terminal is {}x{}.", area.width, area.height);
    text(buf, area.x + 1, area.y + area.height / 2, &msg, AMBER, DESK);
}

// ---------------------------------------------------------------- the gallery

/// Every tile and every part, each shown in a small board with copper running into its pins.
pub fn gallery(buf: &mut Buffer, area: Rect) {
    fill(buf, area, DESK);
    bold(buf, area.x + 1, area.y, "OHM'S WORKSHOP tile gallery", AMBER, DESK);
    text(buf, area.x + 29, area.y, "· every trace mask, then every part, unpowered and powered", DIM, DESK);

    // Traces: all sixteen masks and a jumper, one tile each.
    let mut x = area.x + 1;
    let y = area.y + 2;
    for m in 0..16u8 {
        fill(buf, Rect::new(x, y, TILE_W, TILE_H), BOARD);
        if m == 0 {
            put(buf, x + 2, y + 1, "·", HOLE, BOARD);
        } else {
            draw_copper(buf, x, y, m, mix(COPPER_COLD, COPPER_HOT, m as f64 / 15.0), BOARD);
        }
        x += TILE_W + 1;
    }
    fill(buf, Rect::new(x, y, TILE_W, TILE_H), BOARD);
    draw_jumper(buf, x, y, mix(COPPER_COLD, COPPER_HOT, 0.5), LEAD, BOARD);
    text(buf, area.x + 1, y + 3, "masks 0-15 (none, N, E, NE, ... all four), then a jumper; copper colour cold to hot", DIM, DESK);

    // Parts: each in a strip of three tiles, copper on both pins.
    let r = |o: f64| Kind::Resistor { ohms: o, watts: 0.25 };
    let items: Vec<(&str, Kind, Dir, Look)> = vec![
        ("R 220", r(220.0), Dir::W, Look::default()),
        ("R 4k7", r(4700.0), Dir::W, Look::default()),
        ("R 220 warm", r(220.0), Dir::W, Look { heat: 1.3, ..Look::default() }),
        ("R smoking", r(220.0), Dir::W, Look { heat: 1.9, ..Look::default() }),
        ("R burnt", r(220.0), Dir::W, Look { burnt: true, ..Look::default() }),
        ("R 220 vert", r(220.0), Dir::N, Look::default()),
        ("LED red off", Kind::Led(Colour::Red), Dir::W, Look::default()),
        ("LED red lit", Kind::Led(Colour::Red), Dir::W, Look { glow: 1.0, ..Look::default() }),
        ("LED yellow", Kind::Led(Colour::Yellow), Dir::W, Look { glow: 1.0, ..Look::default() }),
        ("LED green", Kind::Led(Colour::Green), Dir::E, Look { glow: 0.5, ..Look::default() }),
        ("LED blue", Kind::Led(Colour::Blue), Dir::N, Look { glow: 1.0, ..Look::default() }),
        ("LED burnt", Kind::Led(Colour::Red), Dir::W, Look { burnt: true, ..Look::default() }),
        ("lamp off", Kind::Lamp { ohms: 15.0, watts: 1.35 }, Dir::W, Look::default()),
        ("lamp lit", Kind::Lamp { ohms: 15.0, watts: 1.35 }, Dir::W, Look { glow: 1.0, ..Look::default() }),
        ("lamp vert", Kind::Lamp { ohms: 15.0, watts: 1.35 }, Dir::N, Look { glow: 0.6, ..Look::default() }),
        ("battery", Kind::Battery { volts: 4.5, r_int: 0.45 }, Dir::W, Look::default()),
        ("battery vert", Kind::Battery { volts: 4.5, r_int: 0.45 }, Dir::N, Look::default()),
        ("battery flip", Kind::Battery { volts: 4.5, r_int: 0.45 }, Dir::E, Look::default()),
        ("switch off", Kind::Switch { closed: false }, Dir::W, Look::default()),
        ("switch on", Kind::Switch { closed: true }, Dir::W, Look::default()),
        ("switch vert", Kind::Switch { closed: true }, Dir::N, Look::default()),
        ("button", Kind::Button { held: false }, Dir::W, Look::default()),
        ("button held", Kind::Button { held: true }, Dir::W, Look::default()),
        ("cap 100n", Kind::Capacitor { farads: 100e-9 }, Dir::W, Look::default()),
        ("cap 100n vert", Kind::Capacitor { farads: 100e-9 }, Dir::N, Look::default()),
        ("cap 100u", Kind::Capacitor { farads: 100e-6 }, Dir::W, Look::default()),
        ("test point", Kind::TestPoint, Dir::W, Look::default()),
        ("test point N", Kind::TestPoint, Dir::N, Look::default()),
        ("supply", Kind::Supply { volts: 5.0, limit: 0.1 }, Dir::W, Look::default()),
    ];
    let per_row = 6;
    let (cell_w, cell_h) = (16u16, 11u16);
    for (i, (name, kind, facing, look)) in items.into_iter().enumerate() {
        let gx = area.x + 1 + (i % per_row) as u16 * cell_w;
        let gy = area.y + 7 + (i / per_row) as u16 * cell_h;
        fill(buf, Rect::new(gx, gy, TILE_W * 3, TILE_H * 3), BOARD);
        for ty in 0..3 {
            for tx in 0..3 {
                put(buf, gx + tx * TILE_W + 2, gy + ty * TILE_H + 1, "·", HOLE, BOARD);
            }
        }
        let (cx, cy) = (gx + TILE_W, gy + TILE_H);
        let copper = mix(COPPER_COLD, COPPER_HOT, 0.5);
        let horizontal = matches!(facing, Dir::W | Dir::E);
        let pins = if kind == Kind::TestPoint { vec![facing] } else { vec![facing, facing.opposite()] };
        for d in pins {
            let (tx, ty, m) = match d {
                Dir::W => (gx, cy, Dir::E.bit() | Dir::W.bit()),
                Dir::E => (gx + 2 * TILE_W, cy, Dir::E.bit() | Dir::W.bit()),
                Dir::N => (cx, gy, Dir::N.bit() | Dir::S.bit()),
                Dir::S => (cx, gy + 2 * TILE_H, Dir::N.bit() | Dir::S.bit()),
            };
            fill(buf, Rect::new(tx, ty, TILE_W, TILE_H), BOARD);
            draw_copper(buf, tx, ty, m, copper, BOARD);
        }
        let _ = horizontal;
        fill(buf, Rect::new(cx, cy, TILE_W, TILE_H), BOARD);
        draw_part(buf, cx, cy, kind, facing, look, BOARD);
        text(buf, gx, gy + 9, name, TEXT, DESK);
    }
}
