# Brass Knuckles Knock Out for the web

The boxing game from PocketCW, playing in any web browser, with no HamCube. It is the device's
own code: the screens in `firmware/src/ui` and the rules in `logic`, built for WebAssembly.
`src/lib.rs` does what the device's tick and UI task do around them: it runs the keyer, the
decoder, the players and the synth one millisecond at a time, paced by the page's audio.

Build it with `sh build.sh`, which writes `dist/brass-knuckles-knock-out.html`: one file, with
the game embedded, that plays offline. It needs rust-src and nightly features for building
`core` (the script sets `RUSTC_BOOTSTRAP=1`), llvm-objcopy and python3.

| Key | Does |
| - | - |
| Z, X | Dit and dah paddles. Left and right Ctrl too, for a USB paddle adapter. |
| Space or Enter | The dial click. |
| Left and right arrows | Turn the dial. |
| 1 to 4 | F1 to F4. Esc is F4 too. |

The page keeps your fighters, letter statistics and settings in the browser's local storage.
