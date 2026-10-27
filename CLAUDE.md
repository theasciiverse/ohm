# Ohm's Workshop

Asciiverse electronics puzzle game, Rust + Ratatui. Original IP. Short name and every technical
id (folder, crate, binary, repo): `ohm`. A top-down electronics desk: jobs
come into the inbox, you build each gadget on a circuit board from the parts drawers, and an
embedded nodal-analysis solver decides what the circuit actually does. The arc runs from Ohm's law
(a torch) through relays and logic to a small programmable chip. In the family of SHENZHEN I/O,
Retro Gadgets and Turing Complete (see Inspiration), with a real analog solver as its own heart.

**Status** (2026-10-10): chapters 1 to 5 and the first four jobs of 6 playable from a launch page (everything built ships at launch, David 2026-10-27; chapters 4, relays, and 5, diodes and transistors, were built overnight 2026-10-09 to 10
while David slept, not yet seen by him; chapter 6's blinker and one-shot followed on
2026-10-10, and the first chip, a CD4011, the same day): thirty-four jobs that unlock one after another, each with
ticks against the old master's par, the Master's Notebook with twenty-nine checked pages (the meter and scope read its example circuits), the card binder, the
sandbox, help on F1, panels that grow, a save file. David has played the first job and tried
the sandbox; the rest is covered by tests and headless shots, not yet by a person. Sound is in but not yet heard. Own git repo, private at github.com/theasciiverse/ohm (2026-10-09).

```bash
cargo run --release                           # play (--mute for no sound)
RUST_LOG=debug cargo run --release 2>ohm.log  # with a session log
cargo test
cargo run --release -- --bench                # numbers, spec runs and hints as text
cargo run --release -- --shot desk --demo --cells | python3 tools/shot.py play.png
  # --shot title|desk|gallery, --saved (from the save file), --demo builds the lamp, --secs S, --cursor X,Y,
  # --overlay drawers|report|bookN|help|binder, --grow inbox|multimeter|scope|datasheet, --sandbox,
  # --size WxH, --master (the old master's solution on the desk)
  # Shots never write the save file (progress::NO_SAVE): before 2026-10-09 they did, through
  # autosave and job switching, and overwrote David's real save.
```

Save file: `$XDG_DATA_HOME/ohm/progress.txt` (else `~/.local/share/ohm/progress.txt`): the
ledger, solved jobs, each job's best profit and ticks, every job's board as left, the job on
the desk, sound off if turned off, the panel edges. Written two seconds after the last edit
(so a closed window loses at most that), after a test, on switching job or going back to the
launch page, and on quitting. Delete it to start over. `--mute` mutes one run without
saving it.

Code:
- `src/solver.rs`: modified nodal analysis, LEDs by Newton, capacitors by backward Euler, a bench
  supply with current limit, gradual heating and burn-out.
- `src/circuit.rs`: the board (tiles, trace masks, jumpers, parts with facing and burnt state),
  editing, embedding a job in the workbench, the netlist by union-find over tile-edge ports,
  `trace_glyph`, and `Live` (a board as a running circuit, the current in every copper tile).
- `src/bench.rs`: spec cases (`Lit`, `Dark`, `Volts`, `Blown`, and the timed `Lasts`,
  `Reaches` and `Blinks`; several parts with `LED1+LED2`; events partway through, `at 3 set SW1 off`) and
  `diagnose`, the three-step hints.
- `src/job.rs`: the job file format (documented at its top), `jobs/*.txt` compiled in, the
  master's solution, scores and ticks, and a test per job proving its intended answer passes
  and the tempting wrong ones fail.
- `src/book.rs`: the Notebook page format, `book/*.txt` compiled in, tests that every page names
  a source and its worked examples match the solver.
- `src/app.rs`: the game: launch page, jobs and unlocking, editing with undo, the live circuit,
  mouse strokes, panel edges, the drawer, report and Notebook overlays, saving.
- `src/progress.rs`: the save file. `src/input.rs`: terminal events into `Act`s, logged first.
- `src/render.rs`: copper (colour by voltage, ripple for current), parts, the desk and its layout,
  the overlays, the launch page, the gallery.
- `src/audio.rs`: synthesised effects and the hum of current (helpers from SIGPIPE).
- `src/printout.rs`: the printed bench. `src/main.rs`: the loop and the headless shots.
- `tools/shot.py`: cells to PNG; draws box drawing and blocks as shapes, as VTE does.
- `shots/`: every screen, rendered.

## Name

**Ohm's Workshop** (David, 2026-10-08). The game is a workshop: a desk, drawers, customers' jobs
and an old master, and the full name says "puzzle game set somewhere" where plain "Ohm" could be
a tool or a unit converter. In the fiction the town calls the old master "Ohm" and the shop is
named after him, so the name belongs to our character rather than to Georg Ohm. In a Nordic
setting the sign over the door can read "Ohms Verkstad". The apostrophe lives only in titles and
display names, never in ids or file names.

Checked 2026-10-08 by web search: no game, product or shop called "Ohm's Workshop". Nearest:
Ohm's Quest (a steampunk escape-room puzzle console, 2023), Super Ohm (a small Windows circuit
puzzle), OHMIC (an RPG Maker game). Further checks (TMview, Steam, itch.io) skipped: David,
2026-10-09.

## Pillars

1. **The board is the circuit.** There is one grid. The glyphs are computed from it, and the
   netlist is computed from it. Nothing is kept in sync by hand.
2. **Honest electronics.** Real units, real component values and real behaviour. Simplify only
   where it can't change an answer. Anything you learn here should work on a real breadboard.
3. **Always alive.** The circuit runs while you build: current moves, parts glow, relays click.
   A toy first, a test second.
4. **Failure is a show, never a penalty.** Smoke, pops and a melting trace are fun to watch. One
   key puts the burnt parts back.
5. **Solvable by reasoning, findable by poking.** The maths is always available through the
   multimeter and the datasheets, but you never need to do it on paper.
6. **Resizing shows more, never bigger** (David, 2026-10-08). Making the terminal or a panel
   bigger only reveals more content: more of the desk around the board, more history in the
   scope, more lines of a note, more drawers. Nothing is zoomed or scaled: not the board, not
   the parts, not the scope's trace, not the meter's digits.

**Neighbour:** SIGPIPE is real-time pressure against a moving front; Ohm is a solver puzzle with
no timer and no pressure (see `../sigpipe/CLAUDE.md`, "Neighbour to watch").

## What SIGPIPE taught us

SIGPIPE is the favourite of all David's titles with his daughters (David, 2026-10-08). What they
loved, and what it means here:

- **Watching the flow.** Current is drawn as brightness running through the copper, a smooth
  ripple travelling the way the current flows and faster the more of it there is, the way
  SIGPIPE's data runs green through its pipes. No dashes.
- **The sound.** Synthesised clicks, clacks, hum, sizzle and pops are part of the game, not
  polish for later.
- **Short rounds and a bit of pressure.** Jobs stay small. Ohm keeps no timer that can fail you;
  an idea to weigh is a rush job, where the customer waits at the counter and tips for speed.
- **The mouse.** Everything can be played with the mouse alone, without learning a key.

Multiplayer in SIGPIPE hasn't been tried by them yet, so it tells us nothing either way.

## Inspiration

Named by David, 2026-10-08. Store pages checked the same day.

- **SHENZHEN I/O** (Zachtronics, 2016): build devices from components (microcontrollers, memory,
  logic gates, LCDs), program them in a compact assembly language, with a manual of original
  datasheets. Ohm takes the job-based desk, the datasheet binder and, late in the game, a small
  programmable chip.
- **Retro Gadgets** (Licorice and Studio Evil, early access 2022): a pixel-art workbench with parts
  drawers, where you solder boards, add buttons, lights and displays, and program the CPU in Lua.
  Ohm takes the workbench feel: drawers, physical parts, knobs and switches you operate.
- **Turing Complete** (LevelHead, early access 2021): from NAND gates up to arithmetic, memory and
  a full CPU with your own assembly language. Ohm takes the climb from simple parts to a computer.
- **CRUMB Circuit Simulator** (Mike Bushell, 2022): a real-time 3D circuit simulator built to
  feel like working at a workbench, with no goals or levels. Its parts range from passives,
  diodes, BJTs and MOSFETs to logic, timers, op amps and a programmable microcontroller. It shows
  currents and voltages on the circuit and has a four-channel scope. Ohm takes the live,
  honest-physics workbench, and points it at puzzles.

Where Ohm sits: CRUMB has honest physics but no puzzles, and the other three have puzzles built on
logic and code. Ohm puts the puzzles on top of a real analog solver, so a torch, a blinker, a
logic gate and the chip all obey the same physics. And it runs in a terminal.

## Other circuit games and terminal versions (checked 2026-10-08)

**Analog puzzle games are rare.** The only one found is Circuit Warz (2017, free): seven timed
levels on series-parallel networks, RC filters, bridges, op-amp summers, transistor switches and
oscillators, with a Moon Base Alpha alien-attack story; reviews Mixed (29). Electronics Puzzle
Lab (Sagitta Studios, 2024) is a 3D puzzle game with logic gates, and says on its own page that it
is not a precise simulator; one review asks for analog parts. R.E.E.L. is a maze puzzler built
from circuit elements.

**The closest game found: Strip & Rail** (paddylopez, browser, MIT, created 2026-09-27, 0 stars
when checked). A breadboard puzzle game with a real simulator: nodal analysis, Newton iterations
for diodes and LEDs, time steps for capacitors, a behavioural 555, a current-limited bench supply.
Levels are judged by what the circuit does (an LED at 4-25 mA, a blink at 0.5-2 Hz), LEDs burn out
without a resistor, strips glow by voltage, hovering shows readings, and every part is a card in a
Binder. 13 levels, from LED and resistor to a 555 blinker. The difference from Ohm: each level
hands you the schematic and the puzzle is building it on a breadboard, where Ohm hands you a
customer's problem and the puzzle is designing the circuit. It is also a web page, not a
terminal game, and teaches with a proof Rulebook rather than a job story.

BREADBOARD 3D (Philippe Fauteux, Steam, May 2026) is another 3D breadboard sandbox with live
voltage, current and power, a multimeter and a two-channel scope; no levels.

**Digital and logic games are many:** KOHCTPYKTOP (Zachtronics), Silicon Zeroes (PleasingFungus
Games, 2017), MHRD (Funghisoft, 2017, its own hardware description language up to a CPU, a 1980s
startup, Very Positive with 439 reviews), Turing Complete, and the 1980s Learning Company games
Rocky's Boots and Robot Odyssey (from memory, not checked).

**In the terminal there is no circuit game.** What exists (repos read through the GitHub API):

- **CCircuit** (phunanon, C++, Linux, GPL-3.0 in `src/LICENSE`, last push 2018-01-07): a
  terminal logic sandbox inspired by Minecraft redstone. Every cell on a 4096x4096 board is one
  character with an on or off state (`UN_`/`PW_` pairs in `src/globals.hpp`), and power spreads
  from cell to cell; there are no voltages, currents or analog parts, and no goals. Its demos
  show how far it goes: an 8-bit ALU, memory, a 5x7 display and an hh:mm:ss clock with
  7-segment digits. Editing ideas worth having (ideas only, the code is GPL): crossing wires
  bridge themselves, "far-lay" lays a long wire in one move, saved circuits load back as
  components, copy-paste with flips, switches bound to the number keys, slow and fast motion.
- **ASCII-Circuits** (allanwupi, Python, created 2026-09-26, no license): a small parser for a
  text-file schematic language (`+-|` wires, parts named on the drawing, values in a separate
  section) that writes a SPICE netlist for LTspice. It simulates nothing itself (its README names
  its own MNA solver as a "tentative future goal") and is not interactive. Close to Ohm's
  "drawing is the netlist" idea, and to how a job file could carry its board.
- **oombak** (fuad1502, Rust, MIT, active in 2026): a ratatui UI over Verilator for SystemVerilog
  designs, a chip designer's tool and not a game. Its waveform widget
  (`oombak_tui/src/widgets/waveform.rs`) draws logic signals with heavy box lines (`━┃┏┓┗┛`) and
  `╱╲╳` transitions, worth a look for Ohm's scope.
- **Ascii-Circuit-Visualizer** (wiluxgit): an empty repo, a license and a one-line README, pushed
  once in 2021.

None of them combines the terminal, analog physics and puzzles.

## What we take from Strip & Rail

Read 2026-10-08 (commit `a7e4de6`, Patrick Lopez). MIT licence, so code may be adapted with its
copyright notice kept; in practice we write Rust and take the ideas. Its solver is reassuringly
small: union-find nets, dense Gaussian elimination with partial pivoting, SPICE-style `pnjlim`
junction limiting, `GMIN = 1e-9`, a 0.5 ms time step, all in about 250 lines of `src/engine.js`.
That confirms the size estimate for ours.

- **Goals as named kinds that judge behaviour.** Its set (`lit`, `button`, `dimmer`, `blink`,
  `afterglow`, `latch`, `oneshot`, `divider`, `ammeter`, `listen`, plus `all` to chain them) is a
  good starting vocabulary for Ohm's spec cases. Each check returns pass or fail with one plain
  sentence ("LED1 flashes at 0.74 Hz; this level wants 0.5-2 Hz").
- **Diagnosis hints.** When the circuit fails, a hint list ordered most fundamental first (no
  power, a dead short, a part with both legs on one net, a lonely leg, a backwards LED). Each hint
  has three levels (a nudge, the specific problem, the fix) and highlights the holes involved. It
  is built from the board's rules plus one powered trial, so it describes what the build actually
  does. Ohm should have this from the first playable cut.
- **Parts as data.** One file per part: pins with human names ("long leg (anode, +)"), parameters
  with options (LED colours, each with its Vf and display colour), a model kind with its numbers
  (`iMax`, `iBurn`), how it is drawn, and its card. A new part of an existing kind is data only;
  only a new kind needs solver code.
- **Every part is a card.** Cost, type, rules as keywords with reminder text ("**Polarised** (the
  long leg is +)"), stats, a flavour line, collected in a Binder as levels introduce them. A strong
  fit for Ohm's datasheet binder: one card per part, half-block art, opened on the part you hover.
- **A bench supply with a current limit** that switches from constant voltage to constant current
  like a real one. A short becomes a lesson rather than an explosion, and it is a lovely desk
  object.
- **Probe view:** a toggle that labels every net's voltage and every part's current at once.
- **Calculators and proofs that unlock with the levels** (LED resistor, Ohm's law, R x C, divider),
  each with a "why" link to a short proof. For Ohm: the master's notebook.
- **"New in this level"** cards, and a **boss level** that combines earlier stages, briefed in
  stages with "power up after each to check it".
- **Breadboard evidence:** its board (strips, rails, the centre channel) makes a puzzle of its own
  and reads well. That counts toward the open breadboard question.
- **Players' own circuits as levels** (a parts list plus a netlist, or a KiCad export). Later, for
  Ohm's sandbox.

Ohm keeps its own core: a customer's problem to design for, not a schematic to copy.

## The desk

You're the new apprentice at a small electronics repair shop, working at a desk seen from above.
Every job arrives in the inbox as a gadget with a handwritten note from its owner ("the bike lamp
eats batteries", "the doorbell only works when it rains"). The board is the gadget's insides. Some
parts are fixed in place (the battery holder, the bulb socket, the button the customer presses);
you build the rest from the parts drawers.

On the desk:
- **Inbox:** the jobs, each with its note and spec.
- **Board:** the circuit, in the middle.
- **Parts drawers:** what you have for this job, with quantities and prices.
- **Multimeter:** reads V, I and P wherever you point, with a half-block needle that swings
  smoothly.
- **Scope:** a trace over time, for blink rates and delays.
- **Datasheet binder:** the in-game manual, one page per part, opened on the part you hover.

Rough desk layout:

```
┌ INBOX ───────┐┌ BOARD ─────────────────────────┐┌ DRAWER ─┐
│> Mrs Berg    ││ . . . . . . . . . . . . . . .  ││ R  220  │
│  bike lamp   ││ .[+ AA -]══╤══[▌▓▐]══(o)══╗ .  ││ R  1k   │
│  Mr Lund     ││ .          ║              ║ .  ││ LED red │
│  doorbell    ││ .══════════╧════[/ ]══════╝ .  ││ SW      │
│              ││ . . . . . . . . . . . . . . .  ││         │
└──────────────┘└────────────────────────────────┘└─────────┘
┌ MULTIMETER ──┐┌ SCOPE ────────────┐┌ DATASHEETS ──┐
│   4.82 V DC  ││  _/^\_/^\_/^\_    ││ LED, red     │
└──────────────┘└───────────────────┘└──────────────┘
```

**The drawer overlay** (David, 2026-10-08): clicking the DRAWERS title (or `d`) opens every
drawer as a card with its picture, scrollable, over the board and the drawers; picking one (a
click, or arrows and Enter) snaps back to the list with that part in hand. Choosing a part is a
moment of its own, so here an overlay is right.

**Panels grow in place; the board never moves** (David, 2026-10-08). The circuit stays live while
you look at things, so nothing covers the board. Clicking a panel's title (or its key) grows it
along its own strip, and the neighbours it borrows from fold down to their title line; clicking
the title again, or Esc, snaps it back. A folded panel's title stays clickable, to swap.

- **Scope:** across the whole bottom row (about 96 columns instead of 50): a longer time window,
  finer detail. Multimeter and datasheet fold.
- **Datasheet:** up the whole right column: the full card, every rule, live readings, a line on
  why the part behaves that way. Drawers fold.
- **Inbox:** down the whole left column: the full note, the whole spec, the customer's history.
  Multimeter folds.
- **Multimeter:** up the left column: more readings at once (V, I and P of the probed part, more
  probes). Inbox folds.
- **Drawers:** down the right column: every part with its card summary. Datasheet folds.

Built 2026-10-09 for the inbox, multimeter, scope and datasheet, with keys `i`, `v`, `o` and `g`
(Esc also puts a grown panel back before it leaves the desk). The panel edges rest while a panel
is grown. A bigger multimeter lists current, power, V/I and the load against the rating; a
taller datasheet shows every rule, the live readings and a few plain words on why the part
behaves as it does, each block only when it fits whole. The drawers don't grow: their title
already opens the drawer overlay, which shows every part as a card.

Discoverable: each title carries a small `+` or `-`, and the hint row says "click a title to
enlarge it" with the keys. A bigger terminal gives every panel more room on its own, as the other
titles scale to the window; enlarging hands one panel the extra on top of that. **Overlays** are
kept for things that are not panels: the Binder of every card, the test bench report after T, the
job ledger.

- **Score is in the fiction:** the customer pays a fee and the parts come out of it, so profit is
  the cost score. The old master's own solution is the par.
- **Visible payoff:** when the circuit passes, the gadget comes alive on the desk (the lamp
  lights, the doorbell shakes).
- **Light story:** notes, a few regular customers, the master's margin comments in the job ledger.
  No cutscenes.

## Simulation

**Units are real:** V, A, W, Ω, F. Resistors come only in E12 values (1.0, 1.2, 1.5, 1.8, 2.2,
2.7, 3.3, 3.9, 4.7, 5.6, 6.8, 8.2 per decade), which is what makes divider puzzles interesting.

| Part | Model | Chapter |
|---|---|---|
| Trace, crossover | merged into nodes, zero resistance | 1 |
| Battery | ideal source plus internal resistance (a short is survivable and dramatic) | 1 |
| Lamp | fixed resistance at its rated value | 1 |
| Switch, push button, two-way switch | open or closed contact | 1 |
| Resistor | linear, with a power rating: smokes, then burns open | 1 |
| LED (colour sets Vf) | exponential diode via Newton; overcurrent kills it | 1-2 |
| Potentiometer, LDR, thermistor | a pot is two resistors meeting at the wiper (its third pin, a quarter turn clockwise from pin `a`), never below 1 Ω each; an LDR follows the GL5528 (8 to 20 kΩ at 10 lux, gamma 0.7, at least 1 MΩ dark; 15 kΩ chosen at 10 lux, rated 100 mW); a thermistor is 10 kΩ at 25 °C with B 3950 by the B equation | 2 (built) |
| Fuse | 0.1 Ω (chosen); heat follows the square of current over rating with a 0.25 s time constant and it opens at 1.6 times its rating, fitted to IEC 60127-2 fast-acting limits (holds 1.5 times, opens at 2.75 times in 0.05 to 2 s, at 4 times within 0.3 s, at 10 times within 20 ms; a solver test checks each) | 2 (built) |
| Capacitor | backward-Euler companion model. From 1 µF up an electrolytic: polarised, rated in volts, and it vents (opens) when held past its surge voltage (1.15 times its rating) or reversed by more than 1.5 V, both from Vishay's "Aluminum Capacitors: Introduction" (document 25001); its stress follows the square of the overshoot with a 0.5 s time constant (chosen). Below 1 µF a ceramic that never fails. A rebuilt circuit (an edit, a pot turned) keeps every capacitor's charge, matched by tile | 3 (built) |
| Relay | one tile, four legs: the coil along its facing, one contact across, normally open or normally closed (NC). The coil is a resistance from its power (0.36 W: 70 Ω at 5 V, 225 Ω at 9 V, 400 Ω at 12 V); it pulls in at 75 % of its rating and drops out at 5 %, after 8 ms and 5 ms; past 130 % the coil heats (2 s time constant, chosen) and burns open. All from Sanyou's SRD series datasheet (V3.2), taking its limits as the values. The contact moves at the end of a step, so a relay through its own NC contact buzzes (77 Hz). A rebuild keeps every relay's state, by tile. A red stop button (closed until pressed) came with it | 4 (built) |
| Diode | the LED model with a 1N4001's numbers: the Motorola databook's SPICE model (IS 14.11 nA, N 1.984, RS 33.9 mΩ: 0.69 V at 10 mA, 0.96 V at 1 A against Vishay's 1.1 V maximum); rated 1 A, burns at 3 A held (chosen); no reverse breakdown (50 V, past every battery here) | 5 (built) |
| NPN transistor | BC547B by the Ebers-Moll transport model, solved by Newton with junction limiting on both junctions: IS 2.39e-14, BF 294, BR 7.95 (NXP's SPICE model, as posted on EEVblog; BF matches onsemi's typical hFE 290), 625 mW and 100 mA (onsemi), heating by power or collector current like a resistor; its junction capacitances (10 pF and 1.7 pF, onsemi typical) are in, because without them a circuit that flips itself over (a two-transistor blinker) would have to flip in no time and Newton can't follow. A step Newton can't settle is split in halves, down to about a nanosecond. One tile, three legs: collector toward its facing, emitter opposite, base a quarter turn clockwise, like a pot's wiper, or anticlockwise with its legs bent the other way (`e` or a click; `npn flip` in job files), which a symmetric two-transistor circuit needs. Each transistor added has a saturation current 1 % above the last (chosen; real spread is far wider), and a freshly built circuit starts with nanosecond steps growing a tenth at a time: backward Euler at 1 ms damps the fast tipping of a flip-flop and would hold it balanced half on, half off. A rebuild keeps each transistor's junction state, so an edit doesn't make a flip-flop forget | 5 (built) |
| Logic chip (CD4011, built) | four CMOS NAND gates in a 14-pin package spanning 7 by 2 tiles, powered through its own pins (+ on 14, - on 7) like any other part (David, 2026-10-10). Each gate's output joins + or - through 2000 Ω V / supply (TI's CD4011B sheet, SCHS021D: 1 mA at 0.4 V from 5 V, 2.6 mA at 0.5 V from 10 V, typical); an input reads high above half the supply (no hysteresis, as the sheet's transfer curve), takes no current, and has its 5 pF, so with the solver's tiny leak an input wired to nothing keeps its level for seconds. An output past 100 mW (the sheet's limit per output transistor) heats and burns at 1.5 times (chosen); a supply backwards or past 20 V burns the package at once. Gates decide at the end of each 1 ms step, so each gate adds a step of delay (the real one is 60 ns). Not modelled: the input protection diodes (an input driven past the supply passes no current here) and the quiescent current (0.01 µA typical). A rebuild keeps each gate's state, so an edit doesn't make a latch forget | 6 |
| Counter, 7-segment display | behavioural chips on the same footprint, next | 6 |
| Programmable chip | behavioural: reads and drives its pins each step, running a tiny assembly program | 7 |

No AC mains, no inductors as player parts.

**Solver: our own, about 600 lines, no linear-algebra crate.** Plain nodal analysis: batteries
have internal resistance, so each becomes a conductance plus a current source and no extra
unknowns are needed. Dense Gaussian elimination with partial pivoting in f64, under 100 nodes. A
tiny gmin to ground on every node, so floating islands solve cleanly. Newton iteration with
junction-voltage step limiting for nonlinear parts. Backward Euler at a fixed step, sub-stepped
each frame.

Why our own: the game needs one-step-per-frame stepping, game-specific parts (burning, relay
delay, behavioural ICs, the programmable chip) and deterministic tests. Crates surveyed
2026-10-08: sindr (MNA, Newton, backward Euler, MIT/Apache, `0.1.0-alpha.6`, depends on nalgebra;
docs describe end-to-end solves, no documented single-step transient API; source not read), rspice
(AGPL, no nonlinear parts yet), cayenne (2021, tiny), tiny-spice-rs. sindr is the reference to
check our results against.

The solver is a pure module (no ratatui, no clock), tested against textbook answers: a divider,
series and parallel combinations, an RC curve at 63.2% after τ, an LED current, a relay buzzer's
frequency.

## The board

- **A physical board seen from above** (David, 2026-10-08): perfboard dots, copper traces in
  box drawing, parts drawn as small physical objects in half-blocks (resistors with their real
  colour bands, LED domes, battery holders, chips with legs). No schematic view.
- **Tiles are 5x3 characters** (roughly square on screen, a middle row and column for traces).
  At 100x30 (`../../hub/RECORDING.md`) the desk leaves the board 50x18 characters: **10x6 tiles**.
  Puzzles are small on purpose.
- **One workbench, filling the BOARD panel** (David, 2026-10-08). The job's board is laid in the
  middle of one big perfboard (48x26 tiles); the panel is a window onto it. Tiles stay 5x3
  characters at every size: a bigger terminal or panel shows more of the same board, never
  bigger tiles. The job's own area shows slightly brighter holes. Keyboard moves keep the cursor
  in view; a middle-button drag pans (a middle click without dragging clips the probe on).
  (Two first tries were taken out: drawing the board twice as large in big terminals, and a
  small board on a gridded mat, which read as two workbenches.)
- **Panel edges drag with the mouse** (David, 2026-10-08): the seam between the left column and
  the board, between the board and the right column, and between the top and bottom rows. An
  edge lights amber under the mouse and while dragged; right-click puts it back.
- **Erasing is visible** (David asked, 2026-10-08): the ERASE button picks up the eraser (the
  cursor tile turns red; click or drag to remove copper and parts; Esc puts it down), besides
  `x` at the cursor and right-click or right-drag.
- **Cells:** `Empty`, `Blocked` (board shape), `Trace { mask: u8 }` (N/E/S/W bits, like sigpipe's
  `Dir::bit`), `Cross` (two independent nets, a jumper), `Part { id }`. Parts are a list of
  `{ kind, value, pos, rot, fixed }`; two- and three-terminal parts fit in one tile with pins on
  its edges. Chips span several tiles.
- **Netlist:** each tile edge is a port; a trace's mask joins its ports, and union-find merges
  them into nodes. Neighbouring ports connect only when both sides open toward each other; a
  one-sided connection is a dangling end, drawn as a stub and shown as a hint.
- **Trace glyphs come from the mask.** Parts use hand-drawn 5x3 art per rotation. A T-joint
  always connects; a four-way tile is either a junction or a jumper crossing, toggled.
- **Glyph safety:** `▶◀●×` misalign in some terminals (font fallback draws them wide), so the art
  uses box drawing, blocks and ASCII. Unicode's East Asian Width table is no guide here: it lists
  box drawing and half blocks as "ambiguous" too, and every sibling game draws with them. `Ω` is
  also "ambiguous" and not yet checked on screen; `µ` (U+00B5, the micro sign) is narrow by the
  table. Step 3's gallery is where each glyph gets checked by eye.

Powered, the copper's colour shows its voltage (cold copper at 0 V, warm gold at the top), and
current runs through it as a ripple of brightness in the direction it flows, faster for more
current (log scale). The idea of showing both on the wires is Paul Falstad's circuit simulator's.

## Controls

**Mouse and keyboard are both first-class, and work together.** Every action can be done with
either one, and you can mix them freely: the mouse moves the cursor, so a key press acts on
whatever tile the pointer is over (hover a part and press R to rotate it, or hover a switch and
press E to flip it). Neither one is the "real" input with the other added on later.

- **Keyboard:** arrows/WASD/hjkl move. **Shift+move lays trace** behind the cursor. Space places
  the selected part, R rotates, X/Del erases, Tab or 1-9 picks a drawer slot, E operates the part
  under the cursor (flip a switch, hold a button, turn a pot), U/Ctrl-Z undo, Ctrl-Y redo, T test
  bench, Esc back. **No bare `q` quit.**
- **Mouse:** drag lays trace, click places, right click erases, click flips a switch, wheel turns
  a pot. Inbox, drawers, binder and buttons are clickable.
- **Gamepad** (gilrs, SDL-mapped pads only): D-pad moves, hold A lays trace, B erases, shoulders
  cycle the drawer, X rotates, Y operates.
- **Discoverable:** a hint row always shows the keys for the current context, the drawers show
  every part you have, and every change can be undone.
- **Help** (David asked, built 2026-10-09): F1 or `?` anywhere, the `F1 help` label in the desk's
  title bar, or "How to play" on the launch page. `help.txt`, compiled in: `# Heading` lines,
  `keys :: what they do` rows, paragraphs. A centred popup over the dimmed screen, only as big as
  it needs (David asked): the fewest 46-wide columns that hold it all, balanced, keeping each
  section in one column when that fits. Columns never widen, so a bigger terminal shows more of
  them; at 100x30 it fills the screen and arrows, PgUp/PgDn and the wheel scroll. `?` is there because some terminals (GNOME Terminal among them) keep F1 for
  their own help. Keep it true to `src/input.rs` and the mouse code when controls change.

## Feedback and winning

- **Multimeter and scope:** the multimeter reads whatever the cursor (or the pointer) is on.
  The scope does too until a probe is clipped on: `p` at the cursor, the PROBE button then a
  click, or a middle click. Then it watches that part or copper wherever the cursor goes, marked
  with a red P. `p` or PROBE again takes it off.
- **Failures, graded:** a resistor glows dull red, then amber, then smokes (half-block puffs) and
  burns open. An LED flares and goes dark. A shorted battery makes the trace glow, then a fuse
  pops. A reversed electrolytic bangs.
- **Test bench:** each job's spec is a short list of cases. The bench runs them deterministically,
  faster than real time, and shows each case's expected value, measured value and pass or fail,
  with the scope showing time-based specs. Passing pays the fee and the gadget comes to life.
- **Ticks, the reward** (built 2026-10-09, David asked for the challenge, the reward and the
  counting to be clearer). Every job shows up to three ticks, on its paper in the inbox before
  you start and beside its name in the list: **it works** (the spec passes), **parts** (cost no
  more than the old master's), **battery** (lasts at least as long as his, only on jobs where
  `life:` names the case that drains it). The master's own solution is in each job file
  (`master:` rows), and a test proves it passes. Ticks once earned stay.
- **The money:** the first pass pays the fee less the parts. A later build that's cheaper pays
  the difference against your best. The battery tick earns a tip of a tenth of the fee, once.
  The report compares your build with the master's line by line. Saved per job as
  `best ID PROFIT thrifty frugal`.
- **The shelf** (built 2026-10-09): every job is a small half-block gadget on a plank on the
  launch page, dark until solved, then lit with a slow glow, its ticks underneath; jobs not yet
  arrived are sealed letters. Clicking a gadget opens its job. Pictures in `shelf_art`.
- **Later, proposed:** money buys workshop things (bench supply, a second probe, bigger drawers,
  desk decorations; never the learning). For chip jobs, program length as a fourth tick. No server, so no global histograms.
- **Prices with real choices:** a job should offer a dearer easy part and a cheaper clever one.
  In chapter 1 only the café sign does (four 100 Ω 1 W side by side, 1.20 kr, against one
  22 Ω 2 W, 1.50 kr); elsewhere every honest answer costs the same and the parts tick only
  catches waste. A missed tick gets the master's word in the report (`hint-parts:`,
  `hint-battery:` in the job file).

## The Master's Notebook (built 2026-10-09, David asked 2026-10-08)

A guided book of teaching cards about basic electronics, in the fiction the old master's
notebook. Strip & Rail's Rulebook is the reference for the idea (pages that unlock with the
levels, each with its reason); ours is written fresh.

- **A page per idea,** short and plain: what it is, why it matters, a formula only when it earns
  its place, and a small example circuit drawn with the same board tiles, live, so a reader can
  flip its switch and read its meter.
- **Pages unlock as jobs need them,** and a job's note can point to the page it builds on.
  Opened from a NOTEBOOK button or `n`, as an overlay (a moment of its own, like the drawers).
- **First pages, for chapter 1:** current; voltage; resistance and Ohm's law; the closed loop
  and switches; series and parallel; LEDs (polarity, forward voltage, why they need a resistor);
  power and heat (watts and ratings); the resistor colour code; batteries (internal resistance,
  capacity and how long it lasts); measuring (multimeter and scope).
- **Every fact checked** against a real reference before it ships; each page lists its source.
  Checked 2026-10-09: red LED 2.0 V at 20 mA (SparkFun COM-09590), bright green and blue 3.2 V
  (Multicomp Pro 5 mm datasheet), classic GaP green 2.2 V (Kingbright L-5A7GD), the AA and 9 V
  figures above, IEC 60062 for the colour code.
- **Files:** `book/NN-topic.txt`, compiled in like the jobs. Besides text, a page can hold
  `colours:` (the colour code chart, each band on a scrap of resistor body) and `resistor: 4k7`
  (a resistor drawn large with its bands). Opened with `n`, the NOTEBOOK label in
  the inbox's border, or from the launch page. Each page's example circuit runs live beside its
  text, with every part's reading; clicking a switch on the page flips it. Job notes end with the
  pages they build on ("Notebook: LEDs, Ohm's law").

## The launch page (built 2026-10-09, David asked 2026-10-08)

The first screen: the title drawn as copper traces with current rippling through the letters,
a menu: Start or Continue, Sandbox, The Master's Notebook, How to play, Quit (no bare `q`), the
shelf of gadgets underneath, the ledger in one top corner and a clickable sound switch in the
other (`m` too; it left the menu when Sandbox came, as 100x30 has no row to spare). Esc on
the desk with nothing in hand comes back here. Jobs are picked in the inbox (click, or PgUp and
PgDn); each unlocks when the one before is solved, and keeps its board while you visit another.
Everything clickable, keys shown. Fixed-size art centred in the terminal (pillar 6: a bigger
terminal shows more desk round it, never a bigger title).

## The card binder (built 2026-10-09)

One card per kind of part (`BINDER` in `src/app.rs`: battery, bench supply, LED, lamp,
resistor, capacitor, slide switch, push button, fuse, pot, LDR, thermistor, relay, diode,
transistor, logic chip; the stop button shares the push button's card), an overlay with
the list on the left like the Notebook. Opened with `b` at the part under the cursor or in hand,
or from the BINDER label in the datasheet's border. A card is collected once an arrived job uses
that part, or it is on the sandbox board; the standlight brings the capacitor, and the sandbox is
how the bench supply gets collected for now. Each card:
the part's picture, where it was first met, the values the shop stocks, every rule, why it
behaves so (the same text as the datasheet, from `part_facts`) and a line in the old master's
voice. Long cards scroll with the wheel.

## The sandbox (built 2026-10-09, David asked)

A desk with no job: no customer, no spec, no fee and no test, a bare 16x9 stretch of the
workbench, and as many as you like of every kind of part, free, from the start (David: the
sandbox should have every part, not only the ones the jobs have shown). The catalogue in
`Job::sandbox`: AA cell, 4.5 V, 9 V and 12 V batteries, a 5 V bench supply, LEDs in all four
colours, both lamps, E12 resistors from 10 Ω to 100 kΩ plus the café sign's power resistors,
a 100 nF ceramic and electrolytics from 10 µF 25 V to 4700 µF 16 V (and one 1000 µF 6.3 V, to
find out what a rating means), the pot, LDR, thermistor and fuses, a slide switch, a push
button and a stop button, relays at 5, 9 and 12 V, normally open and closed, a 1N4001 diode,
a BC547 transistor and a CD4011. Names read as printed on
real parts ("2.2 kΩ", "1000 µF", lamps by voltage). Its board is saved as `sandbox` and kept
apart from the jobs'; Continue on
the launch page goes back to the job it left. For trying things, for children who just want to
light something and blow it up (pillar 4), and for after the last job; later, players' own
circuits could become jobs from here (the Strip & Rail idea).

## Puzzles

A job is a gadget, a note, a board with its fixed parts and blocked cells, a drawer (parts,
quantities, prices), a fee and the spec cases. Most specs are **ranges over cases** ("LED between
10 and 20 mA for any battery from 4.5 V to 6.0 V"), which keeps the obvious answer from being the
right one.

**Chapter 1 as built** (2026-10-09), each proven by a test in `src/job.rs`:

| Job | Customer | Teaches | Passes | Fails |
|---|---|---|---|---|
| Bike lamp | Mrs Berg | an LED needs a resistor | 150 or 220 Ω, and a switch | 100 Ω (too bright fresh), 1 kΩ (too dim) |
| Porch lamp | Mr Lund | a push button is momentary | a button | a slide switch |
| Two doors | Mrs Ek | parallel is OR | two buttons side by side | two in a row (AND) |
| Doll's house | Elsa, 8 | each LED its own resistor | 150 or 220 Ω per LED | one shared 100 Ω: red hogs, green starves on tired cells (2.6 mA) |
| Christmas star | Mr Nilsson | LEDs in series share one current | three red in series, 220 Ω, a switch | 100 Ω (too bright fresh), 470 Ω (too dim tired) |
| Café sign | Café Ohlsson | power ratings | 22 Ω 2 W; par: four 100 Ω 1 W side by side | 22 Ω 0.25 W burns, 47 Ω burns too |

Battery figures: AA alkaline 2850 mAh at 25 mA, 150 to 300 mΩ fresh (Energizer E91 datasheet);
9 V block 625 mAh (Eveready 522 datasheet). Not from a datasheet, chosen: the 9 V block's 1.5 Ω
inside (its datasheet gives none), the van battery's 0.05 Ω, bulbs as fixed resistances.

**Chapter 2 as built** (2026-10-09), each proven by a test in `src/job.rs`; the pot, LDR and
thermistor are operated with e, a click, or the wheel for a pot, and the bench sets them per case
(`set RV1 0.5`, `set LDR1 1000`, `set TH1 4`); `blown F1` checks a fuse cleared a fault:

| Job | Customer | Teaches | Passes | Fails |
|---|---|---|---|---|
| Boat gauge | Mr Holm, the harbour | a divider, E12 ratios | 4.7k over 2.2k (par, battery tick), 1k over 470 | 2.2k over 1k (0.3125, reads low) |
| Night light | Elsa, 8 | a pot as a variable resistor, with a fixed resistor | 10k pot and 150 or 220 Ω | 100 Ω too bright, the 1k pot can't dim far, no resistor burns the LED |
| Fuel gauge | Mr Holm | a pot as a divider, scaling 12 V to 5 V | 15k above the fixed pot | 10k (6 V), 22k (3.75 V), the pot as a plain variable resistor |
| Light meter | Mr Strand, photographer | an LDR in a divider | LDR on top over 2.2k or 4.7k | 1k, 10k, the LDR underneath |
| Fridge gauge | Café Ohlsson | a thermistor in a divider | thermistor on top over 22k | 10k, 47k, the thermistor underneath |
| Trailer light | Mr Lund | fuses | 2 A | 500 mA blows with the lamp, 5 A holds through the fault, no fuse |

**Chapter 3 as built** (2026-10-09), "Time", each proven by a test in `src/job.rs`; the bench
runs a case through time (`at 3 set SW1 off`), reads at a moment (`lit LED1 0 2m at 0.3`), and
times things from the last event (`lasts LED1 4 8`: stays lit that long; `reaches TP1 3 0.8
1.5`: gets to 3 V in that time):

| Job | Customer | Teaches | Passes | Fails |
|---|---|---|---|---|
| Standlight | Mrs Berg | a capacitor keeps a light going | 4700 µF across the supply (2.8 s) | 2200 µF (1.3 s), any can backwards (vents) |
| Courtesy light | Mr Lund, the van | R x C sets the fade, R the brightness, ratings | 1k and 2200 µF 16 V before the resistor (5.2 s) | 470 too bright, 2.2k too dim, 1000 µF too short, 4700 µF too long, the 10 V can vents on 12 V, the cap across the LED empties in a blink |
| Darkroom sign | Mr Strand | capacitors side by side add up; enough but not too much | two 220 µF side by side | one 220 µF drops out in the bump, 1000 µF lingers 2 s |
| Level crossing | Mr Nilsson, model railway | a delay to a threshold, 1.1 R x C | 10k and 100 µF (1.1 s), 2.2k and 470 µF (dearer) | 4.7k and 100 µF too quick, 22k and 100 µF too slow |
| Crossing lifts | Mr Nilsson | a bleeder empties a capacitor, and divides | 33k across | 22k drops too slowly, 47k lifts too slowly, 10k never reaches 3 V, none never lifts |
| Puppet theatre | Elsa, 8 | the chapter at once: slow up, slow down | many; par 470, 2200 µF, 470 | no charging resistor snaps on, no capacitor snaps both ways, 1k with 2200 µF never bright |

Not a real dynamo: the standlight's battery and switch stand in for one (a real dynamo gives AC
and needs a rectifier, chapter 5).

**Chapter 4 as built** (2026-10-09, overnight, not yet seen by David), "Relays", each proven by
a test in `src/job.rs`; `blinks LP1 0.33 0.5` times a blink or a relay's chatter after it has had
0.5 s to start, over 6 s:

| Job | Customer | Teaches | Passes | Fails |
|---|---|---|---|---|
| Spotlamps | Mr Lund, the van | a relay: a small current switches a big one | a 12 V relay, coil on the thin dash wire (R1, 1.5 Ω 2 W), contact on the battery | wired straight through (dim, the wire burns), NC lights it backwards, the 5 V coil burns |
| Coffee roaster | Café Ohlsson | self-holding: a seal-in contact across START, a stop button that breaks | a 12 V relay holding itself in, the lamp (the motor) beside its coil | no holding contact stops on letting go, an NC relay buzzes |
| Railway signal | Mr Nilsson | NOT: an NC contact | red straight from the track contact, green through a 9 V NC relay | an NO relay gets green backwards |
| Doorbell buzzer | Mrs Ek | a relay through its own NC contact buzzes | a 5 V NC relay (77 Hz) | NO never starts, 12 V can't pull in on 4.5 V, a coil straight off the button just pulls in |
| Cellar light | Mrs Ek | a capacitor across the coil is a timer: 0.4 s per 1000 µF times ln 20 | 4700 µF 16 V (5.6 s) | 2200 µF (2.6 s), the 10 V can vents |
| Lighthouse | Mr Holm, the harbour | a flasher: a buzzer slowed by a capacitor | NC relay, 2200 µF (a flash every 2.5 s) | 1000 µF (1.2 s), 4700 µF, an NO relay never starts |

**Chapter 5 as built** (2026-10-10, overnight, not yet seen by David), "Semiconductors", each
proven by a test in `src/job.rs`. The two-transistor blinker moved to chapter 6: on a 10x6
board its crossed capacitors need a jumper and more room than the other jobs, and it suits the
flip-flops there. The solver already runs it (a test against 2 ln 2 R C):

| Job | Customer | Teaches | Passes | Fails |
|---|---|---|---|---|
| Flash battery box | Mr Strand | a diode in series guards against a battery put in backwards | the diode with its band toward the circuit | no diode (the electrolytic vents on -9 V), the diode backwards |
| Which door | Mrs Ek | diodes as one-way gates | a diode from each door's button to the bell | plain copper: either button lights both LEDs |
| Flood alarm | Mrs Ek | a transistor switches a big current from a tiny one | a BC547 with 470 Ω in its collector (14.6 mA) | 220 too bright, 1k too dim |
| Oil warning | Mr Lund | a transistor as NOT: a base resistor and a switch to minus | 10k into the base, 680 Ω for the LED | 1M too weak, 100 Ω burns while shorted, 470 and 1k |
| Freezer alarm | Café Ohlsson | a thermistor divider on a base | the thermistor over 3.3k | 2.2k too dim at 4 °C, 10k lit in the freezer |
| Jetty light | Mr Holm | an LDR divider on a base: a dusk switch | 47k over the LDR | 10k lit when overcast, 1M never bright |

The oil warning has a cheaper honest answer than the master's: the LED and its resistor with
the switch across the LED, which shorts it out while the engine has pressure (17 mA wasted the
whole time). It passes and beats the par; left in on purpose.

**Chapter 6 begun** (2026-10-10, not yet seen by David), "Logic", as logic from transistors,
the way it was first built (RTL), each proven by a test in `src/job.rs`. The blinker and the
one-shot need a 10x7 board, a row taller than the rest, and two jumpers; at 100x30 the bottom
row scrolls into view. David, 2026-10-10: fine, most players have a bigger terminal. Measured
in the solver, each timed half is 0.62 R C rather than ln 2 R C (0.69), because the LED in the
collector takes a little of the collector's fall; the Notebook says "a little under 0.7":

| Job | Customer | Teaches | Passes | Fails |
|---|---|---|---|---|
| Occupied sign | Café Ohlsson | a flip-flop: two BC547s cross-fed through 10k, LEDs in the emitters, a button grounding each base, one transistor with its legs bent | 10k cross links | 1M can't switch a side on, an unbent transistor's base faces away |
| Crossing lights | Mr Nilsson, model railway | a blinker: capacitors in both cross links | 82k and 10 µF (0.98 flashes a second) | 68k (1.18), 100k (0.81), 100 µF ten times too slow |
| Quiz timer | Elsa, 8 | a one-shot: a capacitor in one cross link, a 47k resistor in the other, the button feeding Q2's base through 10k | 82k and 100 µF (green 5.05 s after letting go) | 68k (4.2 s), 100k (over 6 s), 47 and 220 µF, the button straight to the base burns Q2 |

| Boathouse alarm | Mr Holm, the harbour | the first chip: power on its pins, a NAND with its inputs tied is a NOT, a pull-up, why CMOS: it sips | a CD4011, a 1 MΩ pull-up, 470 Ω for the LED (9 µA shut: 69,000 hours on a 9 V block) | no pull-up (the inputs float and stay low), 10k and 100k drain the battery, the LED across the door contact with no chip works but wastes 19 mA (misses the battery tick), the LED to - lights the wrong way |

Chips are real parts spanning several tiles, powered through their own pins (David, 2026-10-10):
a chip faces the way its notch points; facing north it stands as in its datasheet's top view,
pins 1 to 7 down the left and 8 to 14 back up the right, and it turns (`r`) where it has room.
Placed from the cursor, right and down, on empty holes. In a job file a chip is drawn by
repeating its letter over its tiles (`cd4011` in the legend). The bench names a chip with no
power and an input of a gate in use that reaches nothing but other inputs. Next: a counter
(CD4017) and a 7-segment display with its driver (CD4511), each checked against its datasheet
first.

The launch page's shelf turns a page per chapter (`<` and `>` on the plank, or left and right)
when the terminal can't show every gadget; a wide one shows them all.

Chapters follow the drawers:

1. **Light:** a torch, two bulbs in series vs parallel, a stair light switched from either end
   (two-way switches), "don't fry the LED", an LED that stays in range as the battery runs down.
2. **Measure:** dividers from E12 values only, a battery checker, a fuel gauge, a pot dimmer.
3. **Time:** RC delays, a fade-out courtesy light, a stairwell timer light.
4. **Relays:** a self-holding start/stop circuit, AND/OR/NOT, a doorbell buzzer, a traffic-light
   sequencer.
5. **Semiconductors:** diode protection, a transistor switch, a dusk night light (built; the
   two-transistor blinker moved on to chapter 6).
6. **Logic:** flip-flops, the two-transistor blinker and a one-shot (built), gates, a counter, a 7-segment display.
7. **The chip:** a small programmable chip with its own tiny assembly language, reading and
   driving real pins on the analog board. The finale is a small machine such as an electronic die
   or a reaction game.

**Levels:** `jobs/NN-name.txt` in the moraen/marble style (`key: value` headers for name, note,
fee, drawer and cases, then `grid:` with a character map of fixed parts and blocked cells),
compiled in with `include_str!`, saved under stable ids.

## Technical layout

Same crates as sigpipe: ratatui 0.30, log + env_logger, rodio 0.22 (playback only), gilrs 0.11 as
an optional `gamepad` feature (on by default). No linear-algebra crate.

- `solver.rs`: pure maths
- `circuit.rs`: grid, netlist building, part models
- `chip.rs`: the programmable chip's assembler and interpreter (chapter 7, later)
- `job.rs`: level format and parser
- `bench.rs`: running spec cases
- `render.rs`: desk, board, parts, dots, gadgets
- `input.rs`: the `Act` enum
- `audio.rs`: synthesised relay clicks, switch clacks, 50 Hz hum, sizzle, pops, ka-ching
- `progress.rs`: save data (your solution for every job, not just pass/fail)
- `main.rs`: screens, loop, session logging (raw input, transitions, 500 ms snapshot), headless
  `--shot`

**Stretch:** in an oscillator job, the speaker plays the circuit's simulated waveform itself.

## Build order

Each step is seen working before the next starts. Nothing here needs a repo until David asks.

1. [x] **Solver and its tests** (pure Rust, no UI): resistors, switches, batteries with internal
       resistance, a current-limited bench supply, LEDs by Newton with burn-out, capacitors by
       backward Euler, resistors that heat and burn open. 17 textbook tests (2026-10-08).
2. [x] **Board model, netlist and diagnosis hints** (still headless): tiles, masks, union-find,
       the spec-case checks and the three-level hints from Strip & Rail, a test per hint
       (2026-10-08). A blink check waits for the first part that can blink (relays).
3. [x] **Look before building:** a headless gallery of every part and trace glyph, and the whole
       desk at 100x30, rendered to text and PNG for David to judge. Settles board type, tile size
       and desk layout. David approved the gallery and
       asked to build (2026-10-08), so the look and the 5x3 tiles stay for now.
4. [x] **First playable job, end to end** (2026-10-08): mouse and keyboard editing (drag lays
       copper, right-click erases, wheel rotates, buttons in the board frame), live simulation
       with current rippling through the copper, multimeter, scope, datasheet card, the drawer
       overlay, test bench with hints and pay, sound, session logging. Next: David plays it.
5. [x] **The launch page and the Master's Notebook** (ten pages) (2026-10-09).
6. [x] **Chapter 1**: six jobs, unlocking, the save file (2026-10-09). Still to do from this
       step: the bench supply as a desk object, and David (and his daughters) playing it. The
       card binder: built 2026-10-09. Panels that grow when their title is clicked: built 2026-10-09.
7. [x] **Chapters 2 and 3**: version 1. Chapter 2 built 2026-10-09 (six jobs, four parts, four
       Notebook pages); chapter 3 built 2026-10-09 (six jobs, electrolytics with polarity and
       ratings, timed bench checks, three Notebook pages). Next: David and his daughters play
       version 1.
8. [ ] Later: relays (chapter 4 built 2026-10-09: the relay and the stop button, `blinks`, six
       jobs, four Notebook pages, relay hints, clack and buzz sounds), semiconductors (chapter 5
       built 2026-10-10: the diode and the transistor, six jobs, four pages, their hints),
       logic (chapter 6 begun 2026-10-10: the flip-flop, the blinker and the one-shot, three pages;
       then chips spanning tiles, the CD4011, the boathouse alarm and its page),
       the chip. The sandbox desk: built 2026-10-09.

## Open decisions

- [x] Framing: a top-down electronics desk (David, 2026-10-08)
- [x] Board look: a physical board, no schematic view (David, 2026-10-08)
- [x] Arc: analog first, relays and logic, a programmable chip at the end (David, 2026-10-08)
- [x] Setting: a small Nordic town, early 1980s, prices in kronor (David, 2026-10-09)
- [x] Realism: real units and E12 values throughout (David, 2026-10-09)
- [ ] Desk layout and board size at 100x30
- [x] Scope: everything built ships at launch (David, 2026-10-27), replacing
      "version 1 is chapters 1 to 3, about 18 jobs" (2026-10-09)
- [x] Scope trace: half-block, the house style, no Braille exception (David, 2026-10-09)
- [x] The chip's language: the proposal below (David, 2026-10-10: "I do like it").
- [x] Logic chips are real packages spanning several tiles, with power wired to their pins
      (David, 2026-10-10). Built for the CD4011.
- [x] The programmable chip's program lives in a panel that switches with the drawers (David,
      2026-10-10, below).
- [x] A sandbox desk: yes, built (David, 2026-10-09)
- [x] Board type: perfboard throughout (David, 2026-10-09)
- [x] Name: Ohm's Workshop, short id `ohm` (David, 2026-10-08)
- [x] Name checks (TMview, Steam, itch.io): skipped (David, 2026-10-09)

## The chip (proposal, 2026-10-09)

For chapter 7. In SHENZHEN I/O's spirit (its MC4000 has 9 lines and one register, the MC6000
14 lines and two, checked 2026-10-09), but its pins are honest electronics on the analog board.

- **Pins:** a small chip with + and - (powered from the board like any part) and four pins,
  `p0` to `p3`. Writing makes a pin an output: `1` is the supply voltage, `0` is ground, each
  behind about 25 Ω (chosen, near a real CMOS output), so an LED still needs its resistor and a
  short still heats the chip. Reading a pin gives `1` above half the supply, else `0`. One
  analog input, `a0`, reads 0 to 99 as a share of the supply, for the LDR and thermistor jobs.
- **Registers:** `acc` and `dat`, whole numbers from -999 to 999.
- **Instructions:** `mov A B`, `add A`, `sub A` (into `acc`), `teq A B`, `tgt A B`, `tlt A B`,
  `jmp LABEL`, `slp MS`. A line marked `+` runs only if the last test passed, `-` only if it
  failed. The program starts again from the top when it runs off the end. Only `slp` takes
  time, so every run is deterministic and the bench can test it faster than real time.
- **Size:** 10 lines (labels and comments free). Tight enough to be the puzzle; the fourth
  tick on chip jobs is a program no longer than the master's.
- **No random instruction:** an electronic die counts fast while its button is held, which is
  how real ones work and a better puzzle.

A blinker on `p0` is four lines: `mov 1 p0`, `slp 500`, `mov 0 p0`, `slp 500`.

**The program panel** (David asked, 2026-10-10): the drawers' panel switches between DRAWERS
and CHIP (click either word in its title, or a key; clicking a chip on the board switches to
it), so the board stays in view while you write and the chip's legs change colour as it runs.
About 23 by 18 at 100x30: the ten lines with numbers and `+`/`-` marks take about 16 columns,
leaving rows for the live state (`acc`, `dat`, the pins, the line running). Writing needs its
own mode, since every letter is a desk key: a click in the program or Enter on a chip starts
typing (the border lights), Esc stops, and the circuit keeps running. Clicking the title grows
the panel down the right column like the others (the datasheet folds), with room for comments
beside the code; at its smallest the comments are hidden, not wrapped. With two chips it
shows the one last clicked.
