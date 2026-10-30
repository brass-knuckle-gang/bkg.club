#!/bin/sh
# Builds Brass Knuckles Knock Out for the web: the game's code as WebAssembly, embedded in one
# HTML page, dist/brass-knuckles-knock-out.html, that plays offline in any browser.
# Needs Rust nightly features for building core (RUSTC_BOOTSTRAP=1 below) and rust-src, plus
# llvm-objcopy and python3. Pass extra cargo arguments after the script name.
set -eu
cd "$(dirname "$0")"
RUSTC_BOOTSTRAP=1 cargo build --release --target wasm32-unknown-unknown -Zbuild-std=core "$@"
mkdir -p dist
llvm-objcopy --strip-all target/wasm32-unknown-unknown/release/pocketcw_web.wasm dist/game.wasm
python3 - <<'PY'
import base64
page = open("page.html").read()
wasm = base64.b64encode(open("dist/game.wasm", "rb").read()).decode()
body = page.replace("__WASM_BASE64__", wasm)
# The page as a claude.ai artifact, which adds its own document head, and as a whole
# document for anywhere else: a web host, or a double-click.
open("dist/artifact.html", "w").write(body)
head = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<meta name="description" content="A boxing game that teaches Morse code: key back each opponent's tell to dodge, then counter.">
<link rel="icon" type="image/png" href="favicon.png">
<style>body { margin: 0; }</style>
"""
open("dist/brass-knuckles-knock-out.html", "w").write(head + body.replace("<main>", "</head>\n<body>\n<main>", 1) + "\n</body>\n</html>\n")
PY
ls -l dist/brass-knuckles-knock-out.html dist/artifact.html
