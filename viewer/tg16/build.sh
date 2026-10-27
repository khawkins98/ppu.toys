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

# The built-in demo (the starter toy) plus every example toy, as packs in
# the `ppu tg16 pack` format: pkg/demo.tg16.json and pkg/<name>.tg16.json.
python3 - "$root/crates/tg16-core/toys" "$here/pkg" <<'PY'
import json, os, sys
toys, out = sys.argv[1], sys.argv[2]
def pack(name, path):
    return {"format": "ppu.toys/tg16", "version": 1, "name": name,
            "files": {"main.lua": open(path).read()}, "assets": {}}
for f in sorted(os.listdir(toys)):
    if f.endswith(".lua"):
        name = f[:-4]
        json.dump(pack(name, os.path.join(toys, f)), open(os.path.join(out, name + ".tg16.json"), "w"))
json.dump(pack("sunset", os.path.join(toys, "sunset.lua")), open(os.path.join(out, "demo.tg16.json"), "w"))
PY

echo "built $here/pkg ($(du -h "$here/pkg/tg16_core_bg.wasm" | cut -f1) wasm)"
echo "serve: cd $here && python3 -m http.server 8000  ->  http://localhost:8000/"
