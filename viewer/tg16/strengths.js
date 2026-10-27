// The "TG16 strengths" group of compare.html: pairs written to show where
// the TG16 beats the SNES, and the two places where, on a fair try, it does
// not. compare.js adds STRENGTHS to its picker and calls strengthAbout() and
// strengthStats() for them; everything else about these pairs lives here.
//
// Toys: crates/tg16-core/toys/<id>/ and compare/snes/<id>/ (build.sh packs
// both like the close ports). Each SNES side publishes its own readout in
// `sram.meter` ({ rows: [{ label, value, frac? }], warn? }), the one table
// ppu-core hands back to its host; this page reads it with takeSram().

const GROUP = 'TG16 strengths (and limits)';

const NOTE =
  'Neither engine runs CPU code, so CPU speed is shown by estimated budget meters, not by emulated slowdown. ' +
  'Unique colours are counted live from each screen.';

const PAIRS = [
  {
    id: 'swarm',
    label: 'Swarm: CPU',
    verdict: 'TG16 ahead',
    caption:
      'The HuC6280 runs at 7.16 MHz; the 65816 at 2.68 to 3.58 MHz. Both sides run the same flock (one Lua file, the same on both) and draw the same picture: ' +
      '12 to 52 drones, each pair checked for being too close, about 1,300 checks a frame at the peak. Each side prices that work as hand-written assembly. ' +
      'Per check and per drone the 65816 needs a few cycles fewer (16-bit registers), but the TG16 has about 119,000 cycles a frame to the SNES\'s 49,500 ' +
      '(FastROM code, WRAM data, after DRAM refresh), so the same work takes about half the share of a frame: at 52 drones the TG16 meter reads about 67%, ' +
      'the SNES estimate about 133%, where a real SNES would drop frames. Smarter code (sorting, grids) would help both by the same factor.',
    snes: [
      'Same flock logic (10_swarm.lua); OAM entries with the high-table bits, about 50 cycles each.',
      'A line holds 34 eight-pixel slivers, 17 sixteen-pixel drones, so the same band counting drops the extras (they flicker).',
      'The readout is the toy\'s own 65816 estimate; the costs are listed in compare/snes/swarm/main.lua.',
      'Not used here: the SNES has 128 sprites to the TG16\'s 64.',
    ],
    tg16: [
      'Same flock logic; cpu() charges each check, push and drone update at HuC6280 prices (crates/tg16-core/toys/swarm/main.lua).',
      'The engine adds its own charges: 40 cycles a sprite and 1,500 for vertical blank.',
      'A line holds 16 sprite units; the band counting keeps it there.',
    ],
  },
  {
    id: 'patchwork',
    label: 'Patchwork: palettes',
    verdict: 'TG16 ahead',
    caption:
      'The TG16\'s VCE holds 16 background and 16 sprite palettes of 15 colours, plus the backdrop: 481 colours at once (482 counting the overscan colour). ' +
      'SNES CGRAM holds 256 entries, and in Mode 1 the sprites own the upper 128: 8 palettes each. Here the quilt has 16 colour schemes and each of 16 balloons its own: ' +
      '480 different colours on the TG16 (nearly all of its 512), 240 on the SNES, which repeats its schemes. The SNES picks its 240 from 32,768 colours, ' +
      'and could rewrite palettes between rows with HDMA or blend with colour math; this is what each holds at once.',
    snes: [
      '8 BG palettes: the quilt\'s other 8 schemes reuse them with the ramp turned round (separate tiles), so neighbours still differ.',
      '8 OBJ palettes for 16 balloons: twins, the second mirrored.',
      'Same layout and colours (10_patchwork.lua), exact 9-bit colours in CGRAM.',
    ],
    tg16: [
      'One BG palette per quilt scheme, one sprite palette per balloon.',
      'The colours: all 512, less the 32 darkest, cut by hue into 32 groups of 15.',
    ],
  },
  {
    id: 'reef',
    label: 'Reef: the 512-colour catch',
    verdict: 'SNES ahead',
    caption:
      'The other side of the palette story. Twice the palettes do not help a smooth painting much, because every TG16 entry is one of 512 colours (3 bits a channel): ' +
      'this reef rounds to about 80 distinct colours before any palette is filled, and its water needs dithering to hide the steps. The SNES gets half the palettes ' +
      '(the importer squeezes the reef into 8) but chooses from 32,768 colours, and its water is a free 15-bit HDMA gradient. Both draw 16 fish species; ' +
      'the TG16 gives each its own palette (8-shade body), the SNES pairs them up (4-shade body), and at this size the difference is small.',
    snes: [
      'Mode 1: the reef through the SNES importer, 8 palettes (ordered dither).',
      'Open water: one HDMA channel writes the backdrop colour on every line.',
      'Fish: two species to a sprite palette, 6 colours each plus a shared black and white.',
    ],
    tg16: [
      'image_bg: the reef in 16 palettes, water painted in so the ordered dither smooths the 9-bit steps.',
      'image_sprites: 16 species, a palette each, 15 colours.',
    ],
  },
  {
    id: 'copper',
    label: 'Copper: per-line colour',
    verdict: 'SNES holds its own',
    caption:
      'Per-line effects are where the SNES is strong. The TG16 rewrites 14 palette entries on every line, more than HDMA\'s 8 channels could (CGRAM is writable only in blanking, ' +
      'about one colour per channel per line), but it pays most of the frame for it: 224 raster interrupts and 3,136 palette writes, about 82% of the CPU. ' +
      'The SNES does not race the beam: these stripes only slide, so Mode 2\'s offset-per-tile scrolls a pre-drawn gradient in each column, with no HDMA and almost no CPU, ' +
      'and smoother colours. The TG16\'s real edge is narrower: more than 8 arbitrary colour changes on a line, when they cannot be pre-drawn, and only while the CPU can spare the time.',
    snes: [
      'Mode 2: the colour cycle drawn once as a 256-line gradient (32 tiles, 8 palettes of 15).',
      'Offset-per-tile: 28 words of BG3\'s map give each column its own vertical scroll every frame.',
      'Per-line palette writes would reach 8 of the 14 stripes at most.',
    ],
    tg16: [
      'A raster hook on all 224 lines writes 14 palette entries, left to right ahead of the beam.',
      'Colours: the same 256-step cycle, rounded to 9 bits.',
      'The engine charges 90 cycles an interrupt and 20 a palette write; cpu() adds the table indexing.',
    ],
  },
];

export const STRENGTHS = PAIRS.map((p) => ({
  ...p,
  group: GROUP,
  strength: true,
  snesUrl: `pkg/snes/${p.id}.ppu.json`,
  tg16Url: `pkg/${p.id}.tg16.json`,
}));

const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);

/** The about panel for a strengths pair. */
export function strengthAbout(t) {
  const list = (items) => `<ul>${items.map((s) => `<li>${esc(s)}</li>`).join('')}</ul>`;
  return `<h2>${esc(t.label)} <small style="font-weight:400;color:var(--dim)">(${esc(t.verdict)})</small></h2>
    <p>${esc(t.caption)}</p>
    <p class="hint">${esc(NOTE)} Sources: <code>compare/snes/${esc(t.id)}/</code> and <code>crates/tg16-core/toys/${esc(t.id)}/</code>.</p>
    <div class="cols"><div><h3>SNES</h3>${list(t.snes)}</div><div><h3>TG16</h3>${list(t.tg16)}</div></div>`;
}

// ---------------------------------------------------------------- live rows

const panes = {};
function pane(side) {
  if (panes[side]?.isConnected) return panes[side];
  const stats = document.querySelector(`#${side} .stats`);
  if (!stats) return null;
  const el = document.createElement('div');
  el.className = 'strength-rows';
  el.style.cssText = 'border-top:1px solid var(--line);margin-top:6px;padding-top:4px';
  stats.insertBefore(el, stats.querySelector('ul.msgs'));
  panes[side] = el;
  return el;
}

function rowsHtml(rows, warn) {
  const out = rows.map((r) => {
    const frac = typeof r.frac === 'number' ? r.frac : null;
    const cls = frac === null ? '' : frac > 1 ? 'bad' : frac > 0.8 ? 'warn' : '';
    const bar = frac === null ? '' : `<div class="bar"><i class="${cls}" style="width:${Math.min(1, Math.max(0, frac)) * 100}%"></i></div>`;
    return `<div class="meter"><div class="row"><span>${esc(r.label)}</span><span>${esc(r.value)}</span></div>${bar}</div>`;
  });
  if (warn) out.push(`<div class="meter" style="color:var(--bad);font-size:12px">${esc(warn)}</div>`);
  return out.join('');
}

/** Distinct RGB values in an RGBA frame. */
function uniqueColours(d) {
  const seen = new Set();
  for (let i = 0; i < d.length; i += 4) seen.add((d[i] << 16) | (d[i + 1] << 8) | d[i + 2]);
  return seen.size;
}

let meterCore = null;
let meter = null;

/** Refresh the extra rows; hides them for toys outside the group. */
export function strengthStats(toy, snes, tg) {
  const sp = pane('snes');
  const tp = pane('tg16');
  if (!sp || !tp) return;
  const on = !!toy?.strength;
  sp.hidden = !on;
  tp.hidden = !on;
  if (!on) return;
  const snesRows = [];
  if (snes?.core && !snes.failed) {
    if (meterCore !== snes.core) {
      meterCore = snes.core;
      meter = null;
    }
    try {
      const s = snes.core.takeSram();
      if (s) meter = JSON.parse(s).meter || null;
    } catch {
      // a bad readout only hides these rows
    }
    for (const r of meter?.rows || []) snesRows.push(r);
    snesRows.push({ label: 'Unique colours on screen', value: `${uniqueColours(snes.core.framebuffer())}` });
  }
  sp.innerHTML = rowsHtml(snesRows, meter?.warn);
  const tgRows = [];
  if (tg?.core && !tg.failed) {
    try {
      const b = JSON.parse(tg.core.budget_json());
      if (b.rasterLines) {
        tgRows.push({ label: 'Palette writes per raster line', value: (b.paletteWrites / b.rasterLines).toFixed(1) });
      }
    } catch {
      // the main meters already report a broken budget
    }
    tgRows.push({ label: 'Unique colours on screen', value: `${uniqueColours(tg.core.pixels())}` });
  }
  tp.innerHTML = rowsHtml(tgRows);
}
