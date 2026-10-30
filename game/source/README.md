# Source

The source code of `index.html`: the boxing game from PocketCW and what it needs to run in a
browser.

| Folder | What it is |
| - | - |
| `logic/` | PocketCW's game rules and Morse engine: the fighters, the fight, the keyer, the decoder, the sound synth. |
| `firmware/src/` | PocketCW's screens (`ui/`), drawing, fonts and the game's pixel art (`sprites.rs`). |
| `web/` | The browser build: `src/lib.rs` runs the device's 1 ms tick in the page, `page.html` is the page around it. |
| `tools/` | `spritegen.py` draws `firmware/src/sprites.rs` from the grids in `boxing_art.py`. |

## Build

You need Rust with the `rust-src` component, `llvm-objcopy` and Python 3:

```sh
rustup component add rust-src
cd web
sh build.sh
```

`build.sh` writes `web/dist/brass-knuckles-knock-out.html`. Copy it over `index.html` at the top
of the repository. The script builds Rust's `core` library for WebAssembly from source, which
takes a nightly feature: it sets `RUSTC_BOOTSTRAP=1` so that a stable toolchain can do it.

To change the art, edit `tools/boxing_art.py`, run `python3 tools/spritegen.py`, then build.
