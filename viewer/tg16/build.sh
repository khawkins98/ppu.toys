#!/usr/bin/env bash
# Build the tg16-core wasm module and the demo pack into viewer/tg16/pkg/,
# plus what compare.html needs: the ppu-core (SNES) wasm module in pkg/snes/,
# the SNES ports in compare/snes/ packed for it, and the bundled SNES examples
# with their automatic TG16 conversions in pkg/convert/.
# Needs: rustup target add wasm32-unknown-unknown
#        cargo install wasm-bindgen-cli --version <wasm-bindgen version in Cargo.lock>
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
profile="${PROFILE:-release}"

cd "$root"
cargo build -p tg16-core --target wasm32-unknown-unknown --profile "$profile" --features bindings
# ppu-core's default `bindings` feature is its wasm-bindgen shim (src/wasm.rs).
cargo build -p ppu-core --target wasm32-unknown-unknown --profile "$profile"
# The host `ppu` CLI packs the SNES toys and converts the examples. Always an
# optimized build: `ppu tg16 convert` plays each toy for 12 seconds.
cargo build -p ppu-cli --release

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

mkdir -p "$here/pkg/snes"
wasm-bindgen --target web --no-typescript --out-dir "$here/pkg/snes" \
  "$target_dir/wasm32-unknown-unknown/$profile/ppu_core.wasm"

if command -v wasm-opt >/dev/null; then
  wasm-opt -O2 "$here/pkg/tg16_core_bg.wasm" -o "$here/pkg/tg16_core_bg.wasm"
  wasm-opt -O2 "$here/pkg/snes/ppu_core_bg.wasm" -o "$here/pkg/snes/ppu_core_bg.wasm"
fi

ppu="$target_dir/release/ppu"
# Close ports: compare/snes/<name>/ as SNES packs (`ppu pack`, the format
# Studio opens): pkg/snes/<name>.ppu.json, next to pkg/<name>.tg16.json.
for dir in "$root"/compare/snes/*/; do
  name="$(basename "$dir")"
  "$ppu" pack "$dir" -o "$here/pkg/snes/$name.ppu.json" >/dev/null
done
# Auto-converted examples: each bundled SNES example as pkg/snes/<name>.ppu.json,
# its `ppu tg16 convert` output as pkg/convert/<name>.tg16.json, and the
# converter's report as pkg/convert/<name>.md.
examples="$root/plugin/skills/creating-demos/examples"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$here/pkg/convert"
for dir in "$examples"/*/; do
  [ -f "$dir/ppu.json" ] || continue
  name="$(basename "$dir")"
  "$ppu" pack "$dir" -o "$here/pkg/snes/$name.ppu.json" >/dev/null
  "$ppu" tg16 convert "$dir" -o "$work/$name" >/dev/null
  "$ppu" tg16 pack "$work/$name" -o "$here/pkg/convert/$name.tg16.json" >/dev/null
  cp "$work/$name/CONVERT.md" "$here/pkg/convert/$name.md"
done
# TG16-only toys (an example toy with no compare/snes/<name> port) are listed
# for compare.html in pkg/compare.json.
python3 - "$root/crates/tg16-core/toys" "$root/compare/snes" "$here/pkg/compare.json" <<'PY'
import json, os, sys
toys, ports, out = sys.argv[1:4]
names = sorted(e[:-4] if e.endswith(".lua") else e for e in os.listdir(toys)
               if e.endswith(".lua") or os.path.isfile(os.path.join(toys, e, "main.lua")))
with open(out, "w") as f:
    json.dump({"tg16Only": [n for n in names if not os.path.isdir(os.path.join(ports, n))]}, f)
PY

# The built-in demo (the starter toy) plus every example toy, as packs in
# the `ppu tg16 pack` format (docs/tg16/GUIDE.md, "Packing"):
# pkg/demo.tg16.json and pkg/<name>.tg16.json. An example toy is either a
# single toys/<name>.lua or a directory toys/<name>/ holding main.lua, any
# other top-level *.lua and PNGs in assets/.
python3 - "$root/crates/tg16-core/toys" "$here/pkg" <<'PY'
import base64, json, os, sys
toys, out = sys.argv[1], sys.argv[2]
def write(name, files, assets):
    pack = {"format": "ppu.toys/tg16", "version": 1, "name": name,
            "files": files, "assets": assets}
    with open(os.path.join(out, name + ".tg16.json"), "w") as f:
        json.dump(pack, f, sort_keys=True)
def read(path):
    with open(path) as f:
        return f.read()
for entry in sorted(os.listdir(toys)):
    path = os.path.join(toys, entry)
    if os.path.isfile(path) and entry.endswith(".lua"):
        write(entry[:-4], {"main.lua": read(path)}, {})
    elif os.path.isdir(path) and os.path.isfile(os.path.join(path, "main.lua")):
        files = {f: read(os.path.join(path, f)) for f in sorted(os.listdir(path))
                 if f.endswith(".lua") and os.path.isfile(os.path.join(path, f))}
        assets = {}
        adir = os.path.join(path, "assets")
        if os.path.isdir(adir):
            for f in sorted(os.listdir(adir)):
                if f.lower().endswith(".png") and os.path.isfile(os.path.join(adir, f)):
                    with open(os.path.join(adir, f), "rb") as fh:
                        assets[f] = base64.b64encode(fh.read()).decode("ascii")
        write(entry, files, assets)
write("demo", {"main.lua": read(os.path.join(toys, "sunset.lua"))}, {})
PY

echo "built $here/pkg ($(du -h "$here/pkg/tg16_core_bg.wasm" | cut -f1) TG16 wasm, $(du -h "$here/pkg/snes/ppu_core_bg.wasm" | cut -f1) SNES wasm)"
echo "serve: cd $here && python3 -m http.server 8000  ->  http://localhost:8000/ (and /compare.html)"
