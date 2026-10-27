# Ohm's Workshop

**THE OLD MASTER'S BENCH IS YOURS NOW.**

Welcome to Ohms Verkstad, the little electronics repair shop the whole town
still calls Ohm's. It's the early eighties, you're the new apprentice, and the
inbox is filling up: Mrs Berg's bike lamp eats batteries, Elsa (eight) wants
her doll's house lit, Mr Holm needs a lighthouse flasher for the harbour. Pull
open the parts drawers, lay the copper and watch the circuit come alive while
you build it: current ripples through the traces, LEDs glow, relays click.
Get it wrong and resistors smoke and capacitors go BANG; one key puts the burnt
parts back. Then the test bench decides, and you try to match the old master's
own build.

A real circuit solver underneath (real volts, milliamps, parts and kronor),
from a bike lamp to your first chip, with the Master's Notebook, a multimeter,
a scope and a sandbox. No timer, no pressure. Don't let the magic smoke out!

## Play

With [av](https://asciiverse.sh), the Asciiverse client:

```
av install ohm
av play ohm
```

Or download the archive for your system from this repository's Releases,
unpack it and run `./ohm`.

To build it yourself: `cargo run --release` (`--mute` for no sound).

## Keys

Mouse and keyboard work together: point at a part and press a key to act on it.

| Key | |
|---|---|
| arrows, W A S D, H J K L | move the cursor; with Shift, lay copper |
| mouse drag / click / right click | lay copper / place a part / erase |
| Space / R / X, Del | place / rotate / erase |
| Tab, 1 to 9 | pick a drawer |
| E | operate the part under the cursor |
| T | run the test bench |
| U, Ctrl+Z / Ctrl+Y | undo / redo |
| F1, ? | help, anywhere |
| Esc | back |

Gamepad: d-pad moves, hold A to lay copper, B erases, X rotates, Y operates,
the shoulders change drawer.

Progress is saved in `~/.local/share/ohm/progress.txt`. Session log for bug
reports: `RUST_LOG=debug ohm 2>ohm.log`

## Credits

Every electronics fact is checked against a real datasheet or standard, and
each Notebook page names its source.
