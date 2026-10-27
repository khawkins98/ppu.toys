//! PNG importers: turn RGBA images into TG16 tiles, BAT cells, sprite
//! patterns and palettes.
//!
//! Images arrive as plain RGBA8 (the caller decodes the PNG). Every pixel is
//! rounded to the VCE's 9-bit color first; alpha below 128 is transparent
//! (color index 0). [`import_bg`] and [`import_sprites`] are exact: they
//! never drop or merge colors, and when an image cannot fit (more than 15
//! colors in one tile, more palettes than allowed, too many tiles) they
//! return an [`ImportError`] that says what to change. The `_with` variants
//! take a [`Dither`] and reduce such images instead (see `reduce.rs`); images
//! that already fit come out the same either way.

use std::collections::{BTreeMap, BTreeSet, HashMap};

mod reduce;
pub use reduce::Dither;

use crate::vce::rgb8;

/// Colors a palette can hold besides the transparent color 0.
pub const COLORS_PER_PALETTE: usize = 15;

/// An RGBA8 image, row-major, 4 bytes per pixel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Image, ImportError> {
        let want = width as usize * height as usize * 4;
        if rgba.len() != want {
            return Err(ImportError(format!(
                "{width}x{height} image needs {want} RGBA bytes, got {}",
                rgba.len()
            )));
        }
        Ok(Image {
            width,
            height,
            rgba,
        })
    }

    /// The 9-bit color at `(x, y)`, or `None` when transparent.
    pub fn color(&self, x: u32, y: u32) -> Option<u16> {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        let p = &self.rgba[i..i + 4];
        (p[3] >= 128).then(|| rgb8(p[0], p[1], p[2]))
    }
}

/// Why an image could not be imported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportError(pub String);

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ImportError {}

fn fail<T>(msg: String) -> Result<T, ImportError> {
    Err(ImportError(msg))
}

/// A 16-entry palette. Entry 0 is transparent and always 0 here; the
/// importers never write it to the VCE.
pub type Palette = [u16; 16];

/// What an import lost to fit the hardware.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Quality {
    /// Distinct 9-bit colors in the image.
    pub colors: usize,
    /// Colors were reduced to fit the palettes.
    pub colors_reduced: bool,
    /// Unique tiles merged into similar ones to fit VRAM.
    pub merged_tiles: usize,
    /// Mean distance, in 8-bit RGB units, between each opaque pixel rounded
    /// to 9-bit color and the color the import shows there. 0 when exact.
    pub error: f64,
}

impl Quality {
    /// True when the import is not exact (colors reduced or tiles merged).
    pub fn reduced(&self) -> bool {
        self.colors_reduced || self.merged_tiles > 0
    }
}

/// A background image as tiles, palettes and a map.
#[derive(Clone, Debug, PartialEq)]
pub struct BgImport {
    /// Unique tiles as color indices 0..15, row-major. Tiles are shared
    /// across palettes when their indices match.
    pub tiles: Vec<[u8; 64]>,
    pub palettes: Vec<Palette>,
    /// Map size in tiles.
    pub width: usize,
    pub height: usize,
    /// One `(tile, palette)` per map cell, row-major, indexing `tiles` and
    /// `palettes`.
    pub map: Vec<(u16, u8)>,
    pub quality: Quality,
}

/// A sprite sheet as 16x16 patterns and palettes.
#[derive(Clone, Debug, PartialEq)]
pub struct SpriteImport {
    /// Patterns as color indices, row-major. Frame `i` uses patterns
    /// `i * cells .. (i + 1) * cells`, in the order the VDC reads them for a
    /// sprite of the frame size (left to right, then top to bottom).
    pub patterns: Vec<[u8; 256]>,
    pub palettes: Vec<Palette>,
    /// Palette index (into `palettes`) of each frame.
    pub frame_palettes: Vec<u8>,
    /// 16x16 cells per frame: 1, 2, 4 or 8.
    pub cells: usize,
    pub quality: Quality,
}

impl SpriteImport {
    pub fn frames(&self) -> usize {
        self.frame_palettes.len()
    }
}

/// Fit color sets into as few palettes of 15 colors as a greedy pass can,
/// at most `max`. Returns the palettes (colors sorted) and the palette of
/// each set. `unit` names what a set is ("tile", "frame") for errors.
fn fit_palettes(
    sets: &[BTreeSet<u16>],
    max: usize,
    unit: &str,
    where_: impl Fn(usize) -> String,
) -> Result<(Vec<Palette>, Vec<u8>), ImportError> {
    for (i, s) in sets.iter().enumerate() {
        if s.len() > COLORS_PER_PALETTE {
            return fail(format!(
                "{unit} {} has {} colors after rounding to 9-bit color; a {unit} can use at \
                 most {COLORS_PER_PALETTE} plus transparent",
                where_(i),
                s.len()
            ));
        }
    }
    // Richest sets first so small ones slot into palettes made for big ones.
    let mut order: Vec<usize> = (0..sets.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(sets[i].len()));
    let mut pals: Vec<BTreeSet<u16>> = Vec::new();
    let mut assign = vec![0u8; sets.len()];
    for i in order {
        let s = &sets[i];
        // The palette needing the fewest new colors (ties: lowest index).
        let best = pals
            .iter()
            .enumerate()
            .map(|(p, pal)| (s.difference(pal).count(), p, pal.len()))
            .filter(|&(add, _, len)| len + add <= COLORS_PER_PALETTE)
            .min_by_key(|&(add, p, _)| (add, p));
        let p = match best {
            Some((_, p, _)) => p,
            None => {
                if pals.len() >= max {
                    let all: BTreeSet<u16> = sets.iter().flatten().copied().collect();
                    return fail(format!(
                        "needs more than {max} palette{} of {COLORS_PER_PALETTE} colors \
                         ({} distinct colors after rounding to 9-bit color); use fewer \
                         colors, share colors between {unit}s, or allow more palettes \
                         with pals = N",
                        if max == 1 { "" } else { "s" },
                        all.len()
                    ));
                }
                pals.push(BTreeSet::new());
                pals.len() - 1
            }
        };
        pals[p].extend(s.iter().copied());
        assign[i] = p as u8;
    }
    let palettes = pals
        .iter()
        .map(|set| {
            let mut out = [0u16; 16];
            for (slot, &c) in out[1..].iter_mut().zip(set) {
                *slot = c;
            }
            out
        })
        .collect();
    Ok((palettes, assign))
}

/// Validate a background image's size; returns its size in tiles.
fn bg_size(img: &Image) -> Result<(usize, usize), ImportError> {
    if img.width == 0
        || img.height == 0
        || !img.width.is_multiple_of(8)
        || !img.height.is_multiple_of(8)
    {
        return fail(format!(
            "background images must be a multiple of 8 pixels each way; this one is {}x{}",
            img.width, img.height
        ));
    }
    Ok((img.width as usize / 8, img.height as usize / 8))
}

/// Palettes, the palette of each block and the whole image as palette
/// indices (row-major, 0 transparent).
struct Fitted {
    palettes: Vec<Palette>,
    assign: Vec<u8>,
    index: Vec<u8>,
    colors: usize,
    reduced: bool,
    /// What each pixel should show (row-major, `None` transparent): the
    /// image dithered with all 512 colors when reduced, else its colors.
    reference: Vec<Option<u16>>,
}

/// How an image splits into blocks that each get one palette.
struct Blocks<'a> {
    count: usize,
    /// The block a pixel belongs to.
    block_of: &'a dyn Fn(u32, u32) -> usize,
    /// What a block is, for errors: "tile" or "frame".
    unit: &'a str,
    /// Keep diffused dither error inside each block.
    isolate: bool,
}

/// Give every block (tile or frame) a palette and turn the image into
/// palette indices: exactly when the colors fit, otherwise (with `reduce`)
/// by clustering blocks into `max` reduced palettes and remapping with the
/// chosen dither. `where_` locates a block for errors.
fn fit_blocks(
    img: &Image,
    blocks: &Blocks,
    max: usize,
    where_: impl Fn(usize) -> String,
    reduce: Option<Dither>,
) -> Result<Fitted, ImportError> {
    let &Blocks {
        count: nblocks,
        block_of,
        unit,
        isolate,
    } = blocks;
    let mut hists: Vec<BTreeMap<u16, u32>> = vec![BTreeMap::new(); nblocks];
    for y in 0..img.height {
        for x in 0..img.width {
            if let Some(c) = img.color(x, y) {
                *hists[block_of(x, y)].entry(c).or_default() += 1;
            }
        }
    }
    let colors = hists
        .iter()
        .flat_map(|h| h.keys())
        .collect::<BTreeSet<_>>()
        .len();
    let sets: Vec<BTreeSet<u16>> = hists.iter().map(|h| h.keys().copied().collect()).collect();
    let mut reference = None;
    let (palettes, assign, reduced) = match (fit_palettes(&sets, max, unit, where_), reduce) {
        (Ok((p, a)), _) => (p, a, false),
        (Err(e), None) => return Err(e),
        (Err(_), Some(dither)) => {
            // Build palettes from the colors the dithered image wants.
            let ideal = reduce::ideal(img, dither, block_of, isolate);
            let mut hists: Vec<BTreeMap<u16, u32>> = vec![BTreeMap::new(); nblocks];
            for (i, c) in ideal.iter().enumerate() {
                if let Some(c) = c {
                    let (x, y) = (i as u32 % img.width, i as u32 / img.width);
                    *hists[block_of(x, y)].entry(*c).or_default() += 1;
                }
            }
            let hists: Vec<reduce::Hist> =
                hists.into_iter().map(|h| h.into_iter().collect()).collect();
            let (pals, assign) = reduce::cluster(&hists, max);
            reference = Some(ideal);
            let palettes = pals.iter().map(|p| palette(p)).collect();
            (palettes, assign, true)
        }
    };
    let index = if let (true, Some(dither)) = (reduced, reduce) {
        let lists: Vec<Vec<u16>> = palettes
            .iter()
            .map(|p: &Palette| p[1..].iter().copied().take_while(|&c| c != EMPTY).collect())
            .collect();
        reduce::remap(img, block_of, &assign, &lists, dither, isolate)
    } else {
        let mut index = Vec::with_capacity(img.width as usize * img.height as usize);
        for y in 0..img.height {
            for x in 0..img.width {
                index.push(match img.color(x, y) {
                    None => 0,
                    Some(c) => {
                        let pal = &palettes[assign[block_of(x, y)] as usize];
                        pal[1..].iter().position(|&p| p == c).unwrap() as u8 + 1
                    }
                });
            }
        }
        index
    };
    // Unused slots were marked EMPTY for remap; they hold 0 like before.
    let palettes = palettes
        .into_iter()
        .map(|mut p| {
            for c in p.iter_mut() {
                if *c == EMPTY {
                    *c = 0;
                }
            }
            p
        })
        .collect();
    let reference = reference.unwrap_or_else(|| {
        (0..img.width as usize * img.height as usize)
            .map(|i| img.color(i as u32 % img.width, i as u32 / img.width))
            .collect()
    });
    Ok(Fitted {
        palettes,
        assign,
        index,
        colors,
        reduced,
        reference,
    })
}

/// Marks unused palette slots while fitting (no 9-bit color is this large).
const EMPTY: u16 = 0xffff;

/// A sorted color list as a palette, unused slots EMPTY.
fn palette(colors: &[u16]) -> Palette {
    let mut out = [EMPTY; 16];
    out[0] = 0;
    for (slot, &c) in out[1..].iter_mut().zip(colors) {
        *slot = c;
    }
    out
}

/// Mean distance, in 8-bit RGB, between what each opaque pixel should show
/// (`reference`) and what it does show (`shown(x, y)`), both averaged over
/// the pixel's 3x3 neighbourhood so dither noise that evens out (as it does
/// on a TV) is not counted as error.
fn mean_error(img: &Image, reference: &[Option<u16>], shown: impl Fn(u32, u32) -> u16) -> f64 {
    let (w, h) = (img.width as usize, img.height as usize);
    let want: Vec<Option<reduce::Rgb>> = reference.iter().map(|c| c.map(reduce::expand)).collect();
    let got: Vec<Option<reduce::Rgb>> = (0..w * h)
        .map(|i| want[i].map(|_| reduce::expand(shown((i % w) as u32, (i / w) as u32))))
        .collect();
    let (mut sum, mut n) = (0.0, 0u64);
    for y in 0..h {
        for x in 0..w {
            if want[y * w + x].is_none() {
                continue;
            }
            let (mut a, mut b, mut k) = ([0i32; 3], [0i32; 3], 0);
            for ny in y.saturating_sub(1)..(y + 2).min(h) {
                for nx in x.saturating_sub(1)..(x + 2).min(w) {
                    if let (Some(p), Some(q)) = (want[ny * w + nx], got[ny * w + nx]) {
                        for c in 0..3 {
                            a[c] += p[c];
                            b[c] += q[c];
                        }
                        k += 1;
                    }
                }
            }
            let d: f64 = (0..3)
                .map(|c| ((a[c] - b[c]) as f64 / k as f64).powi(2))
                .sum();
            sum += d.sqrt();
            n += 1;
        }
    }
    if n == 0 {
        0.0
    } else {
        sum / n as f64
    }
}

/// Import a background: the image is cut into 8x8 tiles, each tile gets one
/// of at most `max_palettes` palettes, and identical tiles (same color
/// indices; the BAT has no flip bits) are stored once. Exact: fails when the
/// colors do not fit or more than `max_tiles` unique tiles remain. See
/// [`import_bg_with`] to reduce instead.
pub fn import_bg(
    img: &Image,
    max_palettes: usize,
    max_tiles: usize,
) -> Result<BgImport, ImportError> {
    import_bg_with(img, max_palettes, max_tiles, None)
}

/// [`import_bg`], but with `reduce` set an image that does not fit is made
/// to: when the colors do not fit, tiles are clustered into at most
/// `max_palettes` palettes of 15 reduced colors and remapped with the given
/// dither; when too many unique tiles remain, the most similar ones are
/// merged. Images that fit exactly come out exactly as `import_bg` makes
/// them. [`BgImport::quality`] says what was lost.
pub fn import_bg_with(
    img: &Image,
    max_palettes: usize,
    max_tiles: usize,
    reduce: Option<Dither>,
) -> Result<BgImport, ImportError> {
    let (w, h) = bg_size(img)?;
    let block_of = |x: u32, y: u32| (y / 8) as usize * w + (x / 8) as usize;
    let blocks = Blocks {
        count: w * h,
        block_of: &block_of,
        unit: "tile",
        isolate: false,
    };
    let fit = fit_blocks(
        img,
        &blocks,
        max_palettes,
        |i| format!("at pixel ({}, {})", i % w * 8, i / w * 8),
        reduce,
    )?;
    let iw = img.width as usize;
    let mut tiles: Vec<[u8; 64]> = Vec::new();
    let mut seen: HashMap<[u8; 64], u16> = HashMap::new();
    let mut map = Vec::with_capacity(w * h);
    for (i, &p) in fit.assign.iter().enumerate() {
        let (tx, ty) = (i % w * 8, i / w * 8);
        let px: [u8; 64] = std::array::from_fn(|k| fit.index[(ty + k / 8) * iw + tx + k % 8]);
        let t = *seen.entry(px).or_insert_with(|| {
            tiles.push(px);
            tiles.len() as u16 - 1
        });
        map.push((t, p));
    }
    let mut merged_tiles = 0;
    if tiles.len() > max_tiles {
        let fail_msg = format!(
            "has {} unique tiles but only {max_tiles} fit; repeat more 8x8 blocks exactly \
             (tiles cannot be flipped on the TG16) or use a smaller BAT",
            tiles.len()
        );
        if reduce.is_none() {
            return fail(fail_msg);
        }
        // Cost of drawing tile b's cells with tile a: color error under b's
        // first palette, times how many cells use b.
        let mut first_pal = vec![0u8; tiles.len()];
        let mut uses = vec![0i64; tiles.len()];
        for &(t, p) in map.iter().rev() {
            first_pal[t as usize] = p;
            uses[t as usize] += 1;
        }
        let rgb: Vec<[reduce::Rgb; 16]> = fit
            .palettes
            .iter()
            .map(|p| std::array::from_fn(|i| reduce::expand(p[i])))
            .collect();
        let cost = |b: usize, a: usize, bound: i64| -> Option<i64> {
            let pal = &rgb[first_pal[b] as usize];
            let (tb, ta) = (&tiles[b], &tiles[a]);
            let mut sum = 0i64;
            for k in 0..64 {
                let (ib, ia) = (tb[k] as usize, ta[k] as usize);
                if (ib == 0) != (ia == 0) {
                    return None;
                }
                sum += reduce::d2(pal[ib], pal[ia]) as i64 * uses[b];
                if sum >= bound {
                    return Some(sum);
                }
            }
            Some(sum)
        };
        let target = reduce::merge_tiles(tiles.len(), max_tiles, &cost).ok_or_else(|| {
            ImportError(format!(
                "{fail_msg} (even after merging similar tiles; tiles with different \
                 transparent pixels never merge)"
            ))
        })?;
        let mut new_id = vec![u16::MAX; tiles.len()];
        let mut kept = Vec::new();
        for (t, &to) in target.iter().enumerate() {
            if to == t {
                new_id[t] = kept.len() as u16;
                kept.push(tiles[t]);
            }
        }
        merged_tiles = tiles.len() - kept.len();
        for cell in map.iter_mut() {
            cell.0 = new_id[target[cell.0 as usize]];
        }
        tiles = kept;
    }
    let error = if fit.reduced || merged_tiles > 0 {
        mean_error(img, &fit.reference, |x, y| {
            let (t, p) = map[block_of(x, y)];
            fit.palettes[p as usize][tiles[t as usize][(y % 8 * 8 + x % 8) as usize] as usize]
        })
    } else {
        0.0
    };
    Ok(BgImport {
        tiles,
        palettes: fit.palettes,
        width: w,
        height: h,
        map,
        quality: Quality {
            colors: fit.colors,
            colors_reduced: fit.reduced,
            merged_tiles,
            error,
        },
    })
}

/// Import a sprite sheet of `frame_w` x `frame_h` frames (16 or 32 wide, 16,
/// 32 or 64 tall), read left to right, top to bottom. Each frame is one
/// sprite, so all its cells share a palette. Exact; see
/// [`import_sprites_with`] to reduce colors instead.
pub fn import_sprites(
    img: &Image,
    frame_w: u32,
    frame_h: u32,
    max_palettes: usize,
) -> Result<SpriteImport, ImportError> {
    import_sprites_with(img, frame_w, frame_h, max_palettes, None)
}

/// [`import_sprites`], but with `reduce` set a sheet whose colors do not fit
/// is clustered into at most `max_palettes` palettes of 15 reduced colors
/// (one per frame) and remapped with the given dither. Dithering never
/// carries error from one frame into the next.
pub fn import_sprites_with(
    img: &Image,
    frame_w: u32,
    frame_h: u32,
    max_palettes: usize,
    reduce: Option<Dither>,
) -> Result<SpriteImport, ImportError> {
    if !matches!(frame_w, 16 | 32) || !matches!(frame_h, 16 | 32 | 64) {
        return fail(format!(
            "sprite frames are 16 or 32 wide and 16, 32 or 64 tall, not {frame_w}x{frame_h}"
        ));
    }
    if img.width == 0
        || img.height == 0
        || !img.width.is_multiple_of(frame_w)
        || !img.height.is_multiple_of(frame_h)
    {
        return fail(format!(
            "sprite sheets must be a multiple of the {frame_w}x{frame_h} frame size; this \
             one is {}x{}",
            img.width, img.height
        ));
    }
    let fw = (img.width / frame_w) as usize;
    let frames = fw * (img.height / frame_h) as usize;
    let (cw, ch) = (frame_w / 16, frame_h / 16);
    let cells = (cw * ch) as usize;
    let block_of = |x: u32, y: u32| (y / frame_h) as usize * fw + (x / frame_w) as usize;
    let blocks = Blocks {
        count: frames,
        block_of: &block_of,
        unit: "frame",
        isolate: true,
    };
    let fit = fit_blocks(
        img,
        &blocks,
        max_palettes,
        |i| {
            format!(
                "{i} (at pixel ({}, {}))",
                i % fw * frame_w as usize,
                i / fw * frame_h as usize
            )
        },
        reduce,
    )?;
    let iw = img.width as usize;
    let mut patterns = Vec::with_capacity(frames * cells);
    for f in 0..frames {
        let (fx, fy) = ((f % fw) as u32 * frame_w, (f / fw) as u32 * frame_h);
        for c in 0..cells as u32 {
            let (cx, cy) = ((fx + c % cw * 16) as usize, (fy + c / cw * 16) as usize);
            patterns.push(std::array::from_fn(|k| {
                fit.index[(cy + k / 16) * iw + cx + k % 16]
            }));
        }
    }
    let error = if fit.reduced {
        mean_error(img, &fit.reference, |x, y| {
            let p = fit.assign[block_of(x, y)] as usize;
            fit.palettes[p][fit.index[y as usize * iw + x as usize] as usize]
        })
    } else {
        0.0
    };
    Ok(SpriteImport {
        patterns,
        palettes: fit.palettes,
        frame_palettes: fit.assign,
        cells,
        quality: Quality {
            colors: fit.colors,
            colors_reduced: fit.reduced,
            merged_tiles: 0,
            error,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_wrong_buffer_size() {
        assert!(Image::new(2, 2, vec![0; 15]).is_err());
    }

    #[test]
    fn palette_fit_reuses_supersets() {
        let a: BTreeSet<u16> = [1, 2, 3].into();
        let b: BTreeSet<u16> = [2, 3].into();
        let (pals, assign) = fit_palettes(&[b, a], 4, "tile", |i| i.to_string()).unwrap();
        assert_eq!(pals.len(), 1);
        assert_eq!(assign, vec![0, 0]);
        assert_eq!(&pals[0][..4], &[0, 1, 2, 3]);
    }
}
