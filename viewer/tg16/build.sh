#!/usr/bin/env bash
# Build the tg16-core wasm module and the demo pack into viewer/tg16/pkg/.
# Needs: rustup target add wasm32-unknown-unknown
#        cargo install wasm-bindgen-cli --version <wasm-bindgen version in Cargo.lock>
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
profile="${PROFILE:-release}"

cd "$root"
cargo build -p tg16-core --target wasm32-unknown-unknown --profile "$profile" --features bindings

want="$(awk '/^name = "wasm-bindgen"$/{getline; gsub(/version = |"/, ""); print; exit}' Cargo.lock)"
have="$(wasm-bindgen --version 2>/dev/null | awk '{print $2}')" || true
if [ -z "$have" ]; then
  echo "wasm-bindgen not found: cargo install wasm-bindgen-cli --version $want" >&2
  exit 1
elif [ -n "$want" ] && [ "$have" != "$want" ]; then
  echo "wasm-bindgen CLI is $have but Cargo.lock has $want: cargo install wasm-bindgen-cli --version $want" >&2
  exit 1
fi

target_dir="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
mkdir -p "$here/pkg"
wasm-bindgen --target web --no-typescript --out-dir "$here/pkg" \
  "$target_dir/wasm32-unknown-unknown/$profile/tg16_core.wasm"

if command -v wasm-opt >/dev/null; then
  wasm-opt -O2 "$here/pkg/tg16_core_bg.wasm" -o "$here/pkg/tg16_core_bg.wasm"
fi

# The built-in demo: the starter toy as a pack, in the `ppu tg16 pack` format.
python3 - "$root/crates/tg16-core/toys/sunset.lua" "$here/pkg/demo.tg16.json" <<'PY'
import json, sys
src, out = sys.argv[1], sys.argv[2]
pack = {"format": "ppu.toys/tg16", "version": 1, "name": "sunset",
        "files": {"main.lua": open(src).read()}, "assets": {}}
json.dump(pack, open(out, "w"))
PY

echo "built $here/pkg ($(du -h "$here/pkg/tg16_core_bg.wasm" | cut -f1) wasm)"
echo "serve: cd $here && python3 -m http.server 8000  ->  http://localhost:8000/"
