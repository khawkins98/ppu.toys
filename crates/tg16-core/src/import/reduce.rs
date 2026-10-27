//! Color reduction for the importers: fit an image with more colors than the
//! TG16 can show into a few 15-color palettes, then remap every pixel with
//! optional dithering, and merge near-duplicate tiles when VRAM runs out.
//!
//! Everything here is deterministic: fixed iteration orders, integer pixel
//! math, ties broken by the lowest index. Colors are 9-bit GRB words; the
//! math happens on their 8-bit expansions with luma-ish channel weights
//! (red 2, green 4, blue 3), so green banding costs more than blue.

use std::collections::BTreeMap;

use crate::vce::{rgb8, to_rgb8};

use super::{Image, COLORS_PER_PALETTE};

/// How pixels are matched to a reduced palette.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dither {
    /// Nearest color. Flat areas stay flat; gradients band.
    None,
    /// An 8x8 Bayer pattern, aligned to the image so flat areas still make
    /// identical tiles. The classic console look.
    #[default]
    Ordered,
    /// Floyd–Steinberg error diffusion (serpentine). Smoothest gradients,
    /// but noisy tiles rarely repeat, so it uses more VRAM.
    Diffuse,
}

impl Dither {
    /// Parse the Lua spelling: `"none"`, `"ordered"` or `"fs"`.
    pub fn parse(s: &str) -> Option<Dither> {
        match s {
            "none" => Some(Dither::None),
            "ordered" | "bayer" => Some(Dither::Ordered),
            "fs" | "floyd-steinberg" | "diffuse" => Some(Dither::Diffuse),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Dither::None => "none",
            Dither::Ordered => "ordered",
            Dither::Diffuse => "fs",
        }
    }
}

pub(super) type Rgb = [i32; 3];

/// 8-bit expansion of a 9-bit color.
#[inline]
pub(super) fn expand(c: u16) -> Rgb {
    let [r, g, b] = to_rgb8(c);
    [r as i32, g as i32, b as i32]
}

/// Weighted squared distance between two 8-bit colors.
#[inline]
pub(super) fn d2(a: Rgb, b: Rgb) -> i32 {
    let (r, g, b_) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    2 * r * r + 4 * g * g + 3 * b_ * b_
}

fn to9(c: Rgb) -> u16 {
    let q = |v: i32| v.clamp(0, 255) as u8;
    rgb8(q(c[0]), q(c[1]), q(c[2]))
}

/// Index into `pal` of the color nearest `c` (ties: lowest index).
#[inline]
pub(super) fn nearest(pal: &[Rgb], c: Rgb) -> usize {
    let mut best = (i32::MAX, 0);
    for (i, &p) in pal.iter().enumerate() {
        let d = d2(p, c);
        if d < best.0 {
            best = (d, i);
        }
    }
    best.1
}

/// A weighted color histogram: `(9-bit color, pixel count)`, sorted by color.
pub(super) type Hist = Vec<(u16, u32)>;

fn weighted_mean(h: &[(u16, u32)]) -> u16 {
    let mut s = [0i64; 4];
    for &(c, n) in h {
        let e = expand(c);
        for i in 0..3 {
            s[i] += e[i] as i64 * n as i64;
        }
        s[3] += n as i64;
    }
    let m = |i: usize| ((s[i] + s[3] / 2) / s[3]) as i32;
    to9([m(0), m(1), m(2)])
}

/// Median cut to at most `max` colors: split the box with the widest channel
/// (8-bit range, ties: earliest box, then r, g, b) at its weighted median.
fn median_cut(hist: &[(u16, u32)], max: usize) -> Vec<u16> {
    if hist.len() <= max {
        return hist.iter().map(|&(c, _)| c).collect();
    }
    let mut boxes: Vec<Vec<(u16, u32)>> = vec![hist.to_vec()];
    while boxes.len() < max {
        let mut pick = (0, 0, 0);
        for (bi, b) in boxes.iter().enumerate() {
            if b.len() < 2 {
                continue;
            }
            for ch in 0..3 {
                let (lo, hi) = b.iter().fold((255, 0), |(lo, hi), &(c, _)| {
                    let v = expand(c)[ch];
                    (lo.min(v), hi.max(v))
                });
                // Green counts double: the eye sees it best.
                let range = (hi - lo) * [2, 3, 2][ch];
                if range > pick.2 {
                    pick = (bi, ch, range);
                }
            }
        }
        if pick.2 == 0 {
            break;
        }
        let (bi, ch, _) = pick;
        let mut b = boxes.remove(bi);
        b.sort_by_key(|&(c, _)| (expand(c)[ch], c));
        let total: u64 = b.iter().map(|&(_, n)| n as u64).sum();
        let (mut acc, mut split) = (0u64, b.len() - 1);
        for (i, &(_, n)) in b.iter().enumerate() {
            acc += n as u64;
            if acc * 2 >= total {
                split = (i + 1).min(b.len() - 1);
                break;
            }
        }
        let hi = b.split_off(split);
        boxes.push(b);
        boxes.push(hi);
    }
    let mut out: Vec<u16> = boxes.iter().map(|b| weighted_mean(b)).collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Lloyd iterations: move each palette color to the weighted mean of the
/// histogram colors nearest it.
fn kmeans(hist: &[(u16, u32)], pal: &[u16], iters: usize) -> Vec<u16> {
    let mut pal = pal.to_vec();
    for _ in 0..iters {
        let rgb: Vec<Rgb> = pal.iter().map(|&c| expand(c)).collect();
        let mut groups: Vec<Vec<(u16, u32)>> = vec![Vec::new(); pal.len()];
        for &(c, n) in hist {
            groups[nearest(&rgb, expand(c))].push((c, n));
        }
        let next: Vec<u16> = groups
            .iter()
            .zip(&pal)
            .map(|(g, &old)| if g.is_empty() { old } else { weighted_mean(g) })
            .collect();
        if next == pal {
            break;
        }
        pal = next;
    }
    pal.sort_unstable();
    pal.dedup();
    pal
}

/// Weighted error of drawing `hist` with `pal`.
fn hist_error(hist: &[(u16, u32)], pal: &[Rgb]) -> i64 {
    hist.iter()
        .map(|&(c, n)| {
            let e = expand(c);
            pal.iter().map(|&p| d2(p, e)).min().unwrap_or(i32::MAX / 4) as i64 * n as i64
        })
        .sum()
}

/// Reduce a histogram to at most `max` colors: median cut for the spread,
/// k-means to polish, then refill any slots that rounding to 9-bit merged
/// with the colors drawn worst.
pub(super) fn reduce_palette(hist: &[(u16, u32)], max: usize) -> Vec<u16> {
    if hist.len() <= max {
        return hist.iter().map(|&(c, _)| c).collect();
    }
    let mut pal = kmeans(hist, &median_cut(hist, max), 8);
    while pal.len() < max {
        let rgb: Vec<Rgb> = pal.iter().map(|&c| expand(c)).collect();
        let worst = hist
            .iter()
            .filter(|(c, _)| !pal.contains(c))
            .map(|&(c, n)| {
                (
                    d2(rgb[nearest(&rgb, expand(c))], expand(c)) as i64 * n as i64,
                    c,
                )
            })
            .max_by_key(|&(err, c)| (err, std::cmp::Reverse(c)));
        match worst {
            Some((e, c)) if e > 0 => {
                pal.push(c);
                pal.sort_unstable();
            }
            _ => break,
        }
    }
    pal
}

/// Choose at most `max` palettes of 15 colors for blocks with the given
/// histograms, and the palette of each block. Seeds by farthest point (each
/// new palette is made for the block drawn worst so far), then alternates
/// reassigning blocks to the palette that draws them best and rebuilding
/// each palette from its blocks. Returns sorted palettes and assignments.
pub(super) fn cluster(hists: &[Hist], max: usize) -> (Vec<Vec<u16>>, Vec<u8>) {
    let max = max.max(1);
    let n = hists.len();
    let mut all: BTreeMap<u16, u32> = BTreeMap::new();
    for h in hists {
        for &(c, k) in h {
            *all.entry(c).or_default() += k;
        }
    }
    let all: Hist = all.into_iter().collect();
    let mut pals: Vec<Vec<u16>> = vec![reduce_palette(&all, COLORS_PER_PALETTE)];
    // Per block, the error under the best palette so far.
    let mut best_err: Vec<i64> = vec![i64::MAX; n];
    let mut assign = vec![0u8; n];
    let update = |p: usize, pal: &[u16], best_err: &mut Vec<i64>, assign: &mut Vec<u8>| {
        let rgb: Vec<Rgb> = pal.iter().map(|&c| expand(c)).collect();
        for (i, h) in hists.iter().enumerate() {
            let e = hist_error(h, &rgb);
            if e < best_err[i] {
                best_err[i] = e;
                assign[i] = p as u8;
            }
        }
    };
    update(0, &pals[0], &mut best_err, &mut assign);
    while pals.len() < max {
        let worst = (0..n)
            .filter(|&i| best_err[i] > 0)
            .max_by_key(|&i| (best_err[i], std::cmp::Reverse(i)));
        let Some(w) = worst else { break };
        let pal = reduce_palette(&hists[w], COLORS_PER_PALETTE);
        let p = pals.len();
        update(p, &pal, &mut best_err, &mut assign);
        pals.push(pal);
    }

    for _ in 0..12 {
        // Rebuild every palette from the blocks assigned to it.
        let mut next = Vec::with_capacity(pals.len());
        for (p, old) in pals.iter().enumerate() {
            let mut merged: BTreeMap<u16, u32> = BTreeMap::new();
            for (h, &a) in hists.iter().zip(&assign) {
                if a as usize == p {
                    for &(c, k) in h {
                        *merged.entry(c).or_default() += k;
                    }
                }
            }
            if merged.is_empty() {
                next.push(old.clone());
            } else {
                let h: Hist = merged.into_iter().collect();
                next.push(reduce_palette(&h, COLORS_PER_PALETTE));
            }
        }
        // Reassign every block to the palette that draws it best.
        let rgbs: Vec<Vec<Rgb>> = next
            .iter()
            .map(|p| p.iter().map(|&c| expand(c)).collect())
            .collect();
        let mut changed = next != pals;
        for (i, h) in hists.iter().enumerate() {
            if h.is_empty() {
                continue;
            }
            let (_, p) = rgbs
                .iter()
                .enumerate()
                .map(|(p, rgb)| (hist_error(h, rgb), p))
                .min()
                .unwrap();
            if p != assign[i] as usize {
                assign[i] = p as u8;
                changed = true;
            }
        }
        pals = next;
        if !changed {
            break;
        }
    }

    // Drop palettes no block uses.
    let mut used = vec![false; pals.len()];
    for (h, &a) in hists.iter().zip(&assign) {
        if !h.is_empty() {
            used[a as usize] = true;
        }
    }
    let mut remap = vec![0u8; pals.len()];
    let mut kept = Vec::new();
    for (p, pal) in pals.into_iter().enumerate() {
        if used[p] {
            remap[p] = kept.len() as u8;
            kept.push(pal);
        }
    }
    if kept.is_empty() {
        kept.push(Vec::new());
    }
    for (h, a) in hists.iter().zip(assign.iter_mut()) {
        *a = if h.is_empty() { 0 } else { remap[*a as usize] };
    }
    (kept, assign)
}

/// Classic 8x8 Bayer matrix, 0..=63.
const BAYER: [[i32; 8]; 8] = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21],
];

/// Strength of the ordered pattern: one 9-bit step (255 / 7), so a value
/// between two levels dithers between them and a value on a level stays
/// on it. The same everywhere, so neighbouring tiles dither alike.
const BAYER_SPREAD: i32 = 36;

/// Dither the image pixel by pixel. `pick(x, y, want)` chooses the code
/// written for a pixel whose (dithered) target color is `want` and returns
/// it with the color it shows. `block_of` and `isolate` keep diffused error
/// inside each block when set. Transparent pixels get code 0 and pass no
/// error on.
fn dither_with(
    img: &Image,
    dither: Dither,
    block_of: &dyn Fn(u32, u32) -> usize,
    isolate: bool,
    pick: &mut dyn FnMut(u32, u32, Rgb) -> (u16, Rgb),
) -> Vec<u16> {
    let (w, h) = (img.width as usize, img.height as usize);
    let src = |x: usize, y: usize| -> Option<Rgb> {
        let i = (y * w + x) * 4;
        let p = &img.rgba[i..i + 4];
        (p[3] >= 128).then(|| [p[0] as i32, p[1] as i32, p[2] as i32])
    };
    let mut out = vec![0u16; w * h];
    // Diffused error in 1/16ths for this row and the next.
    let mut err = vec![[[0i32; 3]; 2]; w];
    for y in 0..h {
        for e in err.iter_mut() {
            e[0] = e[1];
            e[1] = [0; 3];
        }
        let rtl = dither == Dither::Diffuse && y % 2 == 1;
        for k in 0..w {
            let x = if rtl { w - 1 - k } else { k };
            let Some(c) = src(x, y) else { continue };
            let want = match dither {
                Dither::None => c,
                Dither::Ordered => {
                    let o = (2 * BAYER[y & 7][x & 7] + 1 - 64) * BAYER_SPREAD / 128;
                    [c[0] + o, c[1] + o, c[2] + o]
                }
                Dither::Diffuse => {
                    let e = err[x][0];
                    [
                        (c[0] + e[0] / 16).clamp(0, 255),
                        (c[1] + e[1] / 16).clamp(0, 255),
                        (c[2] + e[2] / 16).clamp(0, 255),
                    ]
                }
            };
            let (code, got) = pick(x as u32, y as u32, want);
            out[y * w + x] = code;
            if dither != Dither::Diffuse {
                continue;
            }
            let q = [want[0] - got[0], want[1] - got[1], want[2] - got[2]];
            let b = isolate.then(|| block_of(x as u32, y as u32));
            let dir: isize = if rtl { -1 } else { 1 };
            for (dx, row, wgt) in [(dir, 0, 7), (-dir, 1, 3), (0, 1, 5), (dir, 1, 1)] {
                let nx = x as isize + dx;
                if nx < 0 || nx >= w as isize || y + row >= h {
                    continue;
                }
                if b.is_some_and(|b| block_of(nx as u32, (y + row) as u32) != b) {
                    continue;
                }
                let slot = &mut err[nx as usize][row];
                for ch in 0..3 {
                    slot[ch] += q[ch] * wgt;
                }
            }
        }
    }
    out
}

/// The image dithered to the full 512-color space: what each pixel would
/// show with every 9-bit color available. Palettes are built from these
/// colors, so a tile whose dithered colors fit keeps its dither exactly.
/// `None` for transparent pixels, row-major.
pub(super) fn ideal(
    img: &Image,
    dither: Dither,
    block_of: &dyn Fn(u32, u32) -> usize,
    isolate: bool,
) -> Vec<Option<u16>> {
    let codes = dither_with(img, dither, block_of, isolate, &mut |_, _, want| {
        let c = to9(want);
        (c + 1, expand(c))
    });
    codes.into_iter().map(|c| c.checked_sub(1)).collect()
}

/// Remap the image to palette indices (0 transparent, 1..=15 the colors of
/// the pixel's palette). `block_of(x, y)` names the block a pixel belongs to
/// and `pal_of[block]` its palette. With `isolate`, diffused error never
/// crosses into another block (each sprite frame is separate).
pub(super) fn remap(
    img: &Image,
    block_of: &dyn Fn(u32, u32) -> usize,
    pal_of: &[u8],
    palettes: &[Vec<u16>],
    dither: Dither,
    isolate: bool,
) -> Vec<u8> {
    let rgbs: Vec<Vec<Rgb>> = palettes
        .iter()
        .map(|p| p.iter().map(|&c| expand(c)).collect())
        .collect();
    let codes = dither_with(img, dither, block_of, isolate, &mut |x, y, want| {
        let pal = &rgbs[pal_of[block_of(x, y)] as usize];
        if pal.is_empty() {
            return (0, want);
        }
        let i = nearest(pal, want);
        (i as u16 + 1, pal[i])
    });
    codes.into_iter().map(|c| c as u8).collect()
}

/// Merge the most similar tiles until at most `max` remain. `cost(b, a)` is
/// the error of drawing tile `b`'s cells with tile `a`'s pixels, or `None`
/// when they cannot share (different transparency). Returns, per tile, the
/// tile it now uses (itself when kept), or `None` when even merging every
/// compatible pair cannot get under `max`.
pub(super) fn merge_tiles(
    n: usize,
    max: usize,
    cost: &dyn Fn(usize, usize, i64) -> Option<i64>,
) -> Option<Vec<usize>> {
    let mut target: Vec<usize> = (0..n).collect();
    let mut alive = n;
    while alive > max {
        let live: Vec<usize> = (0..n).filter(|&i| target[i] == i).collect();
        // Each live tile's cheapest live partner.
        let mut cands: Vec<(i64, usize, usize)> = Vec::new();
        for &b in &live {
            let mut best: Option<(i64, usize)> = None;
            for &a in &live {
                if a == b {
                    continue;
                }
                let bound = best.map_or(i64::MAX, |(c, _)| c);
                if let Some(c) = cost(b, a, bound) {
                    if c < bound {
                        best = Some((c, a));
                    }
                }
            }
            if let Some((c, a)) = best {
                cands.push((c, b, a));
            }
        }
        if cands.is_empty() {
            return None;
        }
        // Cheapest first; on ties, later tiles go first, so the first
        // occurrence of a pattern is the one kept.
        cands.sort_unstable_by_key(|&(c, b, a)| (c, std::cmp::Reverse(b), a));
        let before = alive;
        // Merge in order of cost; a tile that was merged into, or merged
        // away, this round waits for the next round's fresh costs.
        let mut touched = vec![false; n];
        for (_, b, a) in cands {
            if alive <= max {
                break;
            }
            if touched[a] || touched[b] {
                continue;
            }
            target[b] = a;
            touched[a] = true;
            touched[b] = true;
            alive -= 1;
        }
        if alive == before {
            return None;
        }
    }
    // Resolve chains to a surviving tile.
    for i in 0..n {
        let mut t = i;
        while target[t] != t {
            t = target[t];
        }
        target[i] = t;
    }
    Some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduce_palette_keeps_small_histograms() {
        let h = vec![(1, 3), (8, 1), (64, 9)];
        assert_eq!(reduce_palette(&h, 15), vec![1, 8, 64]);
    }

    #[test]
    fn reduce_palette_respects_budget_and_is_deterministic() {
        let h: Hist = (0..512).map(|c| (c as u16, 1 + c % 7)).collect();
        let a = reduce_palette(&h, 15);
        assert!(a.len() <= 15 && a.len() >= 12, "{a:?}");
        assert_eq!(a, reduce_palette(&h, 15));
    }

    #[test]
    fn cluster_separates_disjoint_blocks() {
        // Two groups of blocks with unrelated colors, two palettes allowed.
        let reds: Hist = (0..8).map(|r| (crate::vce::grb(r, 0, 0), 8)).collect();
        let blues: Hist = (0..8).map(|b| (crate::vce::grb(0, 7, b), 8)).collect();
        let hists = vec![reds.clone(), blues.clone(), reds, blues];
        let (pals, assign) = cluster(&hists, 2);
        assert_eq!(pals.len(), 2);
        assert_eq!(assign[0], assign[2]);
        assert_eq!(assign[1], assign[3]);
        assert_ne!(assign[0], assign[1]);
    }

    #[test]
    fn merge_picks_cheapest_pairs() {
        // Tiles 0 and 1 are near-identical, 2 is far from both.
        let v = [0i64, 1, 100];
        let m = merge_tiles(3, 2, &|b, a, _| Some((v[b] - v[a]).abs())).unwrap();
        assert_eq!(m.iter().filter(|&&t| t == 2).count(), 1);
        assert_eq!(m[0], m[1]);
        assert!(merge_tiles(3, 2, &|_, _, _| None).is_none());
    }
}
