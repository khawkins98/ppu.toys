//! PNG importers: turn RGBA images into TG16 tiles, BAT cells, sprite
//! patterns and palettes.
//!
//! Images arrive as plain RGBA8 (the caller decodes the PNG). Every pixel is
//! rounded to the VCE's 9-bit color first; alpha below 128 is transparent
//! (color index 0). The importers are exact: they never drop or merge colors.
//! When an image cannot fit (more than 15 colors in one tile, more palettes
//! than allowed, too many tiles), they return an [`ImportError`] that says
//! what to change.

use std::collections::{BTreeSet, HashMap};

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

/// A background image as tiles, palettes and a map.
#[derive(Clone, Debug, PartialEq, Eq)]
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
}

/// A sprite sheet as 16x16 patterns and palettes.
#[derive(Clone, Debug, PartialEq, Eq)]
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

/// Map a block of 9-bit colors (None = transparent) to palette indices.
fn indices(colors: &[Option<u16>], pal: &Palette) -> Vec<u8> {
    colors
        .iter()
        .map(|c| match c {
            None => 0,
            Some(c) => pal[1..].iter().position(|p| p == c).unwrap() as u8 + 1,
        })
        .collect()
}

/// Import a background: the image is cut into 8x8 tiles, each tile gets one
/// of at most `max_palettes` palettes, and identical tiles (same color
/// indices; the BAT has no flip bits) are stored once. Fails when more than
/// `max_tiles` unique tiles remain.
pub fn import_bg(
    img: &Image,
    max_palettes: usize,
    max_tiles: usize,
) -> Result<BgImport, ImportError> {
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
    let (w, h) = (img.width as usize / 8, img.height as usize / 8);
    let blocks: Vec<Vec<Option<u16>>> = (0..w * h)
        .map(|i| {
            let (tx, ty) = ((i % w) as u32 * 8, (i / w) as u32 * 8);
            (0..64).map(|p| img.color(tx + p % 8, ty + p / 8)).collect()
        })
        .collect();
    let sets: Vec<BTreeSet<u16>> = blocks
        .iter()
        .map(|b| b.iter().flatten().copied().collect())
        .collect();
    let (palettes, assign) = fit_palettes(&sets, max_palettes, "tile", |i| {
        format!("at pixel ({}, {})", i % w * 8, i / w * 8)
    })?;
    let mut tiles: Vec<[u8; 64]> = Vec::new();
    let mut seen: HashMap<[u8; 64], u16> = HashMap::new();
    let mut map = Vec::with_capacity(w * h);
    for (b, &p) in blocks.iter().zip(&assign) {
        let px: [u8; 64] = indices(b, &palettes[p as usize]).try_into().unwrap();
        let t = *seen.entry(px).or_insert_with(|| {
            tiles.push(px);
            tiles.len() as u16 - 1
        });
        map.push((t, p));
    }
    if tiles.len() > max_tiles {
        return fail(format!(
            "has {} unique tiles but only {max_tiles} fit; repeat more 8x8 blocks exactly \
             (tiles cannot be flipped on the TG16) or use a smaller BAT",
            tiles.len()
        ));
    }
    Ok(BgImport {
        tiles,
        palettes,
        width: w,
        height: h,
        map,
    })
}

/// Import a sprite sheet of `frame_w` x `frame_h` frames (16 or 32 wide, 16,
/// 32 or 64 tall), read left to right, top to bottom. Each frame is one
/// sprite, so all its cells share a palette.
pub fn import_sprites(
    img: &Image,
    frame_w: u32,
    frame_h: u32,
    max_palettes: usize,
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
    // Per frame: its cells, each a block of 256 colors.
    let frame_cells: Vec<Vec<Vec<Option<u16>>>> = (0..frames)
        .map(|f| {
            let (fx, fy) = ((f % fw) as u32 * frame_w, (f / fw) as u32 * frame_h);
            (0..cells as u32)
                .map(|c| {
                    let (cx, cy) = (fx + c % cw * 16, fy + c / cw * 16);
                    (0..256)
                        .map(|p| img.color(cx + p % 16, cy + p / 16))
                        .collect()
                })
                .collect()
        })
        .collect();
    let sets: Vec<BTreeSet<u16>> = frame_cells
        .iter()
        .map(|f| f.iter().flatten().flatten().copied().collect())
        .collect();
    let (palettes, frame_palettes) = fit_palettes(&sets, max_palettes, "frame", |i| {
        format!(
            "{i} (at pixel ({}, {}))",
            i % fw * frame_w as usize,
            i / fw * frame_h as usize
        )
    })?;
    let patterns = frame_cells
        .iter()
        .zip(&frame_palettes)
        .flat_map(|(f, &p)| {
            let pal = palettes[p as usize];
            f.iter()
                .map(move |c| <[u8; 256]>::try_from(indices(c, &pal)).unwrap())
        })
        .collect();
    Ok(SpriteImport {
        patterns,
        palettes,
        frame_palettes,
        cells,
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
