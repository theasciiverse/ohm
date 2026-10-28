# Ohm's Workshop

**THE OLD MASTER'S BENCH IS YOURS NOW.**

Welcome to Ohms Verkstad, the little electronics repair shop the whole town
still calls Ohm's. It's the early eighties, you're the new apprentice, and the
inbox is filling up: Mrs Berg's bike lamp keeps blowing its bulb, Elsa (eight)
wants her doll's house lit, the harbour club's model lighthouse has to flash.
Pull open the parts drawers, lay the copper and watch the circuit come alive
while you build it: current ripples through the traces, LEDs glow, relays
click. Get it wrong and resistors smoke and capacitors vent with a crack; one
key puts the burnt parts back. Then the test bench decides, and you try to
match the old master's own build.

A real circuit solver underneath (real volts, milliamps, parts and kronor), 34
jobs from a bike lamp to your first chip, with the Master's Notebook, a
multimeter, a scope and a sandbox. No timer, no pressure. Don't let the magic
smoke out!

## Play

With [av](https://asciiverse.sh), the Asciiverse client:

```
av install ohm
av play ohm
```

To build it yourself: `cargo run --release` (`--mute` for no sound).

## Keys

Mouse and keyboard work together: point at a part and press a key to act on it.

| Key | |
|---|---|
| arrows | move the cursor; with Shift, lay copper |
| C | lay copper with plain arrows; C again stops |
| mouse drag / click / right click | lay copper / place a part / erase |
| middle drag | move round the workbench |
| Space, Enter / R / X, Del | place / rotate / erase |
| Tab, 0 to 9 / D | pick a drawer (0 is copper) / every drawer as cards |
| E | operate the part under the cursor |
| P | clip the scope's probe on, or take it off |
| T / F | run the test bench / mend burnt parts |
| U, Ctrl+Z / Ctrl+Y | undo / redo |
| N / B | the Master's Notebook / the card binder |
| PgUp / PgDn | the previous / next job |
| M | sound on or off |
| F1, ? | help, anywhere |
| Esc / Ctrl+Q | back / quit |

Progress is saved in `~/.local/share/ohm/progress.txt`. Session log for bug
reports: `RUST_LOG=debug ohm 2>ohm.log`

## Credits

Every electronics fact is checked against a real datasheet or standard, and
each Notebook page names its source.

## Licence

GNU General Public License, version 3 or (at your option) any later
version; see [LICENSE](LICENSE).
