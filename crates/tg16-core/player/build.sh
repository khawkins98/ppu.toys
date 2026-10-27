#!/bin/sh
# Rebuild player.bin from player.s (needs cc65: ca65 and ld65).
# The Rust exporter embeds player.bin, so cargo builds never need cc65;
# `cargo test -p tg16-core` checks the committed blob against this source
# whenever ca65 is installed.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out=${1:-"$here/player.bin"}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
ca65 --cpu huc6280 -o "$tmp/player.o" "$here/player.s"
ld65 -C "$here/player.cfg" -o "$out" "$tmp/player.o"
