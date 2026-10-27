// SNES vs TG16: one toy on both engines, side by side, on one clock.
// The SNES side is ppu-core (pkg/snes/), the TG16 side tg16-core (pkg/).
// build.sh makes both wasm modules and every pack this page loads.
import initTg16, { Tg16 } from './pkg/tg16_core.js';
import initSnes, { PpuCore } from './pkg/snes/ppu_core.js';

const FPS = 60;
const STEP_MS = 1000 / FPS;
const RATE = { snes: 32000, tg16: 44100 }; // each engine's audio() sample rate

// ---------------------------------------------------------------- the toys

const PORTS = [
  {
    id: 'sunset',
    label: 'Sunset',
    snes: [
      'Mode 1: hills on BG1 and ground on BG2, two real layers scrolling at their own speeds.',
      'The ground shears line by line: an HDMA table on BG2\'s horizontal scroll.',
      'Sky: a per-line CGRAM write to the backdrop, so the TG16\'s 8 bands become a smooth gradient.',
      'The middle ball has OBJ priority 0, behind BG1. Music: the same tune on the S-DSP\'s built-in lead, bass and hat samples.',
    ],
    tg16: [
      'One background layer and three 32x32 sprites.',
      'Raster hooks: 8 backdrop writes for the sky bands, and a per-line scroll for the faster ground (parallax from one layer).',
      'The middle ball is a sprite behind the background (front = false). Music: PSG pulse lead, triangle bass, noise hi-hat.',
    ],
  },
  {
    id: 'kit_demo',
    label: 'Kit demo',
    snes: [
      'Parallax: BG2 mountains, BG1 hills and BG3 (2bpp) clouds, each with its own scroll register.',
      'Road: an HDMA hook switches lines 128-199 to Mode 7 and re-aims the matrix each line, so the road really has perspective.',
      'Ghost: a sprite in OBJ palette 4, added onto the sub screen by colour math. Steady, no flicker.',
      'Letterbox: colour window 1 spans the masked lines and CGWSEL clips them to black. Music on the S-DSP.',
    ],
    tg16: [
      'kit.parallax: per-band scroll on the one background layer.',
      'kit.floor: soft Mode 7, a BYR reload and a sideways shift on every road line (no scaling).',
      'kit.sprite_layer clouds behind the layer; kit.flicker_spr ghost (every other frame).',
      'kit.mask_lines letterbox: layers off and a flat backdrop on those lines. Music on the PSG.',
    ],
  },
  {
    id: 'photo',
    label: 'Photo',
    snes: [
      'The same 512x224 painting through the SNES importer. One layer can address only 1024 tiles and the photo needs about 1400, so the left half is BG1 and the right half BG2 (own character bases, 4 palettes each, 8 in all).',
      'HDMA writes both scroll registers for the parallax and the lake ripple.',
      'Fixed-colour subtraction darkens the reflection a little.',
      'Music: flute arpeggio with software vibrato and the echo unit, string pad.',
    ],
    tg16: [
      'The painting on one layer, reduced to 16 palettes of 15 colours (ordered dither) by image_bg.',
      'Raster splits: sky, hills, and a line-by-line ripple on the reflection.',
      'Music: sine arpeggio with LFO vibrato, triangle pad.',
    ],
  },
];
const EXAMPLES = ['colour-math', 'finale', 'hdma', 'mode7', 'windows'];

// TG16-only toys come from pkg/compare.json (build.sh) and are added in main().
const TOYS = [
  ...PORTS.map((p) => ({ ...p, group: 'Close ports', snesUrl: `pkg/snes/${p.id}.ppu.json`, tg16Url: `pkg/${p.id}.tg16.json` })),
  ...EXAMPLES.map((id) => ({
    id,
    label: id,
    group: 'Auto-converted SNES examples',
    snesUrl: `pkg/snes/${id}.ppu.json`,
    tg16Url: `pkg/convert/${id}.tg16.json`,
    notesUrl: `pkg/convert/${id}.md`,
  })),
];

// ---------------------------------------------------------------- state

const $ = (id) => document.getElementById(id);
let tgWasm;
let toy = null;
let snes = null; // { core, pack, failed }
let tg = null; // { core, files, images, failed }
let frameNo = 0;
let paused = false;
let acc = 0;
let last = 0;
let hear = 'snes';
let audio = null;
let padMask = { snes: 0, tg16: 0 };
let loadSeq = 0;

function errorText(e) {
  if (e && typeof e === 'object') {
    if (e.message && e.file) return `${e.file}${e.line ? `:${e.line}` : ''}: ${e.message}`;
    if ('message' in e) return e.message;
  }
  return String(e);
}

function msg(side, text, kind = 'warn') {
  const ul = $(`${side}-msgs`);
  if ([...ul.children].some((li) => li.textContent === text)) return;
  const li = document.createElement('li');
  li.className = kind;
  li.textContent = text;
  ul.append(li);
  while (ul.children.length > 8) ul.firstChild.remove();
  if (kind === 'error') console.warn(`[${side}] ${text}`);
}

// ---------------------------------------------------------------- loading

function b64ToBytes(b64) {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

async function decodeImage(name, bytes) {
  const bmp = await createImageBitmap(new Blob([bytes], { type: 'image/png' }), {
    premultiplyAlpha: 'none',
    colorSpaceConversion: 'none',
  });
  const c = document.createElement('canvas');
  c.width = bmp.width;
  c.height = bmp.height;
  const g = c.getContext('2d', { willReadFrequently: true });
  g.drawImage(bmp, 0, 0);
  const data = g.getImageData(0, 0, bmp.width, bmp.height).data;
  bmp.close();
  return { name, width: c.width, height: c.height, rgba: new Uint8Array(data.buffer) };
}

async function fetchJson(url) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  return res.json();
}

async function loadTg16Pack(url) {
  const pack = await fetchJson(url);
  if (pack.format !== 'ppu.toys/tg16') throw new Error(`${url} is not a TG16 pack`);
  // Lua load order: names sorted, main.lua last (GUIDE.md, "Packing").
  const files = Object.entries(pack.files || {}).sort(([a], [b]) =>
    a === 'main.lua' ? 1 : b === 'main.lua' ? -1 : a < b ? -1 : a > b ? 1 : 0,
  );
  const images = [];
  for (const [name, b64] of Object.entries(pack.assets || {})) {
    if (/\.png$/i.test(name)) images.push(await decodeImage(name.replace(/\.[^.]*$/, ''), b64ToBytes(b64)));
  }
  return { files, images };
}

async function loadSnesPack(url) {
  const pack = await fetchJson(url);
  if (pack.version !== 'ppu.toys/1') throw new Error(`${url} is not a ppu.toys/1 pack`);
  return {
    files: pack.files.map((f) => ({ name: f.name, source: f.source })),
    sources: pack.sources.map((s) => ({ name: s.name, payload: b64ToBytes(s.payload) })),
    title: pack.title,
  };
}

/** Fresh engines, both at frame 0. Either side may fail on its own. */
function bootMachines() {
  frameNo = 0;
  acc = 0;
  audio?.flush();
  $('snes-msgs').replaceChildren();
  $('tg16-msgs').replaceChildren();
  if (snes) {
    snes.core?.free();
    snes.failed = false;
    try {
      const core = new PpuCore();
      snes.core = core;
      for (const s of snes.pack.sources) {
        const r = core.addSource(s.name, s.payload);
        if (!r.ok) msg('snes', `source ${s.name}: ${r.error}`, 'error');
      }
      const r = core.setSources(snes.pack.files);
      if (!r.ok) throw r.error;
    } catch (e) {
      snes.failed = true;
      msg('snes', errorText(e), 'error');
    }
  }
  if (tg) {
    tg.core?.free();
    tg.failed = false;
    try {
      const core = new Tg16();
      tg.core = core;
      for (const im of tg.images) core.add_image(im.name, im.width, im.height, im.rgba);
      core.load(JSON.stringify(tg.files));
      for (const w of JSON.parse(core.warnings_json())) msg('tg16', w);
    } catch (e) {
      tg.failed = true;
      msg('tg16', errorText(e), 'error');
    }
  }
  runFrame();
  draw();
  showStats(true);
  status();
}

async function selectToy(id) {
  const next = TOYS.find((t) => t.id === id) || TOYS[0];
  const seq = ++loadSeq;
  toy = next;
  $('toy').value = toy.id;
  $('snes-name').textContent = toy.snesUrl ? toy.snesUrl.replace(/^.*\//, '') : '(none)';
  $('tg16-name').textContent = toy.tg16Url.replace(/^.*\//, '');
  history.replaceState(null, '', `?toy=${encodeURIComponent(toy.id)}`);
  showAbout(toy);
  const [s, t] = await Promise.allSettled([
    toy.snesUrl ? loadSnesPack(toy.snesUrl) : Promise.resolve(null),
    loadTg16Pack(toy.tg16Url),
  ]);
  if (seq !== loadSeq) return; // a newer pick won
  snes?.core?.free();
  tg?.core?.free();
  snes = s.status === 'fulfilled' && s.value ? { pack: s.value, core: null, failed: false } : null;
  clearSnes(toy.snesUrl ? '' : 'TG16 only: no SNES version of this toy.');
  tg = t.status === 'fulfilled' ? { ...t.value, core: null, failed: false } : null;
  bootMachines();
  if (s.status === 'rejected') msg('snes', errorText(s.reason), 'error');
  if (t.status === 'rejected') msg('tg16', errorText(t.reason), 'error');
}

/** Blank the SNES screen, with a caption when there is nothing to show. */
function clearSnes(caption) {
  const g = ctx.snes;
  g.fillStyle = '#000';
  g.fillRect(0, 0, 256, 224);
  if (caption) {
    g.fillStyle = '#9a9ca4';
    g.font = '10px system-ui, sans-serif';
    g.textAlign = 'center';
    g.fillText(caption, 128, 112);
  }
}

// ---------------------------------------------------------------- about panel

function esc(s) {
  return s.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
}
function inline(s) {
  return esc(s)
    .replace(/`([^`]+)`/g, '<code>$1</code>')
    .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
}

/** Just enough Markdown for CONVERT.md: headings, lists, tables, paragraphs. */
function markdown(text) {
  const out = [];
  const lines = text.split('\n');
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (!line.trim()) continue;
    let m;
    if ((m = line.match(/^(#{1,3}) (.*)/))) {
      const n = m[1].length + 1;
      out.push(`<h${n}>${inline(m[2])}</h${n}>`);
    } else if (line.startsWith('|')) {
      const rows = [];
      while (i < lines.length && lines[i].startsWith('|')) rows.push(lines[i++]);
      i--;
      const cells = (r) => r.replace(/^\||\|$/g, '').split('|').map((c) => c.trim());
      const body = rows.filter((r) => !/^\|[\s:|-]+\|$/.test(r));
      out.push('<table>' + body.map((r, k) =>
        `<tr>${cells(r).map((c) => (k ? `<td>${inline(c)}</td>` : `<th>${inline(c)}</th>`)).join('')}</tr>`).join('') + '</table>');
    } else if (/^[-*] /.test(line)) {
      const items = [];
      while (i < lines.length && /^[-*] /.test(lines[i])) items.push(lines[i++].slice(2));
      i--;
      out.push(`<ul>${items.map((it) => `<li>${inline(it)}</li>`).join('')}</ul>`);
    } else {
      const para = [];
      while (i < lines.length && lines[i].trim() && !/^(#|\||[-*] )/.test(lines[i])) para.push(lines[i++]);
      i--;
      out.push(`<p>${inline(para.join(' '))}</p>`);
    }
  }
  return out.join('\n');
}

async function showAbout(t) {
  const el = $('about');
  if (!t.snesUrl) {
    el.innerHTML = `<h2>${esc(t.label)}: TG16 only</h2><p>A TurboGrafx-16 toy with no SNES port yet (<code>crates/tg16-core/toys/${esc(t.id)}</code>). Add one as <code>compare/snes/${esc(t.id)}/</code> and it moves to the close ports.</p>`;
  } else if (t.notesUrl) {
    el.innerHTML = `<h2>${esc(t.label)}: automatic conversion</h2><p>Left: the bundled SNES example as written. Right: <code>ppu tg16 convert</code> run on it at build time, unedited. The converter's notes:</p><div id="notes">loading…</div>`;
    try {
      const res = await fetch(t.notesUrl);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const md = await res.text();
      if (toy === t) $('notes').innerHTML = markdown(md);
    } catch (e) {
      if (toy === t) $('notes').textContent = `could not load ${t.notesUrl}: ${errorText(e)}`;
    }
  } else {
    const list = (items) => `<ul>${items.map((s) => `<li>${esc(s)}</li>`).join('')}</ul>`;
    el.innerHTML = `<h2>${esc(t.label)}: close port</h2>
      <p>The same toy written twice, each the way its hardware does it. Sources: <code>compare/snes/${esc(t.id)}/</code> and <code>crates/tg16-core/toys/${esc(t.id)}</code>.</p>
      <div class="cols"><div><h3>SNES</h3>${list(t.snes)}</div><div><h3>TG16</h3>${list(t.tg16)}</div></div>`;
  }
}

// ---------------------------------------------------------------- running

function runFrame() {
  const t = frameNo / FPS;
  if (snes && !snes.failed) {
    try {
      snes.core.setPad(padMask.snes);
      snes.core.frame(t, frameNo);
      if (audio && hear === 'snes') audio.push(snes.core.audio(), RATE.snes);
    } catch (e) {
      snes.failed = true;
      msg('snes', errorText(e), 'error');
    }
  }
  if (tg && !tg.failed) {
    try {
      tg.core.set_pad(padMask.tg16);
      tg.core.frame(t, frameNo);
      if (audio && hear === 'tg16') audio.push(tg.core.audio(), RATE.tg16);
    } catch (e) {
      tg.failed = true;
      msg('tg16', errorText(e), 'error');
    }
  }
  frameNo++;
}

const ctx = { snes: $('snes-screen').getContext('2d'), tg16: $('tg16-screen').getContext('2d') };

function draw() {
  if (snes?.core && !snes.failed) {
    const px = snes.core.framebuffer();
    ctx.snes.putImageData(new ImageData(new Uint8ClampedArray(px.buffer, px.byteOffset, px.length), 256, 224), 0, 0);
  }
  if (tg?.core && !tg.failed) {
    const w = tg.core.width();
    const h = tg.core.height();
    const c = $('tg16-screen');
    if (c.width !== w || c.height !== h) {
      c.width = w;
      c.height = h;
      fit();
    }
    const px = new Uint8ClampedArray(tgWasm.memory.buffer, tg.core.pixels_ptr(), tg.core.pixels_len());
    ctx.tg16.putImageData(new ImageData(px, w, h), 0, 0);
  }
}

function bar(id, frac) {
  const el = $(id);
  el.style.width = `${Math.min(1, Math.max(0, frac)) * 100}%`;
  el.className = frac > 1 ? 'bad' : frac > 0.8 ? 'warn' : '';
}

let statTick = 0;
function showStats(force) {
  if (!force && ++statTick % 6) return;
  if (snes?.core && !snes.failed) {
    try {
      const ov = snes.core.objOverflow();
      $('s-spr').textContent = `${ov.maxSprites} / 32${ov.rangeOver ? ' (range over)' : ''}`;
      bar('sb-spr', ov.maxSprites / 32);
      $('s-sliv').textContent = `${ov.maxTiles} / 34${ov.timeOver ? ' (time over)' : ''}`;
      bar('sb-sliv', ov.maxTiles / 34);
      const dsp = snes.core.dspState();
      const on = dsp.voices.filter((v) => v.envx > 0).length;
      $('s-voices').textContent = `${on} / 8`;
      bar('sb-voices', on / 8);
      $('s-echo').textContent = dsp.echo.delay ? `${dsp.echo.delay * 2} KB of 64 (delay ${dsp.echo.delay})` : 'off';
      const m7 = snes.core.m7Scanlines();
      let lines = 0;
      for (let y = 0; y < m7.length; y += 4) if (!Number.isNaN(m7[y])) lines++;
      $('s-m7').textContent = `${lines} / 224`;
      const map = snes.core.memoryMap();
      const words = map.vram.reduce((n, r) => n + (r.end - r.start), 0);
      $('s-vram').textContent = `${words} of 32768 words`;
    } catch (e) {
      msg('snes', `stats: ${errorText(e)}`);
    }
  }
  if (tg?.core && !tg.failed) {
    let b;
    try {
      b = JSON.parse(tg.core.budget_json());
    } catch {
      return;
    }
    $('t-cpu').textContent = `${(b.cpu * 100).toFixed(1)}%`;
    bar('tb-cpu', b.cpu);
    $('t-raster').textContent = `${b.rasterLines}`;
    $('t-spr').textContent = `${b.sprites} / 64`;
    bar('tb-spr', b.sprites / 64);
    $('t-units').textContent = `${b.peakLineUnits} / 16${b.droppedLines ? ` (dropped on ${b.droppedLines} lines)` : ''}`;
    bar('tb-units', b.peakLineUnits / 16 + (b.droppedLines ? 0.01 : 0));
    $('t-vram').textContent = `${b.vramWords}`;
    // Toys that model their own CPU work with cpu(cycles) report it separately.
    $('t-soft-row').hidden = b.software === undefined;
    if (b.software !== undefined) {
      const frac = b.software / (b.cyclesPerFrame || 119318);
      $('t-soft').textContent = `${b.software} (${(frac * 100).toFixed(1)}%)`;
      bar('tb-soft', frac);
    }
    for (const p of b.problems) msg('tg16', p);
  }
}

let fps = 0;
let fpsFrames = 0;
let fpsT = 0;
function status() {
  const state = paused ? 'paused' : 'running';
  const side = (m, name) => (!m ? `${name}: not loaded` : m.failed ? `${name}: stopped (error)` : null);
  const errs = [side(snes, 'SNES'), side(tg, 'TG16')].filter(Boolean);
  const snd = audio ? `sound: ${hear.toUpperCase()} (${audio.ctx.sampleRate} Hz out)` : 'sound off';
  $('status').textContent = `frame ${frameNo} · t ${(frameNo / FPS).toFixed(2)} s · ${state} · ${fps} fps · ${snd}${errs.length ? ' · ' + errs.join(' · ') : ''}`;
}

function tick(now) {
  requestAnimationFrame(tick);
  if (!last) last = now;
  const dt = now - last;
  last = now;
  if (!paused && (snes || tg)) {
    acc += Math.min(dt, 100);
    let ran = 0;
    while (acc >= STEP_MS && ran < 4) {
      runFrame();
      acc -= STEP_MS;
      ran++;
    }
    if (ran >= 4) acc = 0;
    if (ran) {
      draw();
      showStats(false);
      fpsFrames += ran;
    }
  }
  if (now - fpsT >= 500) {
    fps = Math.round((fpsFrames * 1000) / (now - fpsT));
    fpsFrames = 0;
    fpsT = now;
    status();
  }
}

function step() {
  if (!paused) return;
  runFrame();
  draw();
  showStats(true);
  status();
}

// ---------------------------------------------------------------- audio

// One output for both machines; each chunk says how fast it was sampled, so
// switching A/B only changes which engine's chunks are sent.
const WORKLET = `
class CompareOut extends AudioWorkletProcessor {
  constructor() {
    super();
    this.q = []; this.cur = null; this.pos = 0; this.queued = 0;
    this.port.onmessage = (e) => {
      if (e.data === 'flush') { this.q = []; this.cur = null; this.pos = 0; this.queued = 0; return; }
      if (this.queued > sampleRate * 0.15) { this.q = []; this.queued = 0; } // bound the latency
      this.q.push(e.data); this.queued += e.data.pcm.length / 2 / e.data.ratio;
    };
  }
  process(_in, outputs) {
    const L = outputs[0][0]; const R = outputs[0][1] || L;
    for (let i = 0; i < L.length; i++) {
      if (!this.cur || this.pos >= this.cur.pcm.length / 2) {
        if (this.cur) { this.queued -= this.cur.pcm.length / 2 / this.cur.ratio; this.pos -= this.cur.pcm.length / 2; }
        this.cur = this.q.shift() || null;
        if (!this.cur) { this.pos = 0; L[i] = 0; R[i] = 0; continue; }
        if (this.pos < 0) this.pos = 0;
      }
      const j = Math.floor(this.pos) * 2;
      L[i] = this.cur.pcm[j]; R[i] = this.cur.pcm[j + 1];
      this.pos += this.cur.ratio;
    }
    return true;
  }
}
registerProcessor('compare-out', CompareOut);
`;

function toFloat(pcm) {
  const f = new Float32Array(pcm.length);
  for (let i = 0; i < pcm.length; i++) f[i] = pcm[i] / 32768;
  return f;
}

async function toggleSound() {
  if (audio) {
    await audio.ctx.close();
    audio = null;
    $('sound').textContent = '▶ Sound on';
    $('sound').classList.add('primary');
    status();
    return;
  }
  const ac = new AudioContext({ latencyHint: 'interactive' });
  if (ac.audioWorklet) {
    const url = URL.createObjectURL(new Blob([WORKLET], { type: 'text/javascript' }));
    await ac.audioWorklet.addModule(url);
    URL.revokeObjectURL(url);
    const node = new AudioWorkletNode(ac, 'compare-out', { numberOfInputs: 0, outputChannelCount: [2] });
    node.connect(ac.destination);
    audio = {
      ctx: ac,
      push: (pcm, rate) => {
        const f = toFloat(pcm);
        node.port.postMessage({ pcm: f, ratio: rate / ac.sampleRate }, [f.buffer]);
      },
      flush: () => node.port.postMessage('flush'),
    };
  } else {
    // ScriptProcessor fallback for plain-http origins other than localhost.
    const sp = ac.createScriptProcessor(1024, 0, 2);
    let q = [];
    let cur = null;
    let pos = 0;
    sp.onaudioprocess = (e) => {
      const L = e.outputBuffer.getChannelData(0);
      const R = e.outputBuffer.getChannelData(1);
      for (let i = 0; i < L.length; i++) {
        if (!cur || pos >= cur.pcm.length / 2) {
          if (cur) pos -= cur.pcm.length / 2;
          cur = q.shift() || null;
          if (!cur) { pos = 0; L[i] = R[i] = 0; continue; }
        }
        const j = Math.floor(pos) * 2;
        L[i] = cur.pcm[j];
        R[i] = cur.pcm[j + 1];
        pos += cur.ratio;
      }
    };
    sp.connect(ac.destination);
    audio = {
      ctx: ac,
      push: (pcm, rate) => {
        if (q.length > 9) q = [];
        q.push({ pcm: toFloat(pcm), ratio: rate / ac.sampleRate });
      },
      flush: () => { q = []; cur = null; },
    };
  }
  await ac.resume();
  $('sound').textContent = '■ Sound off';
  $('sound').classList.remove('primary');
  status();
}

function setHear(side) {
  hear = side;
  $('hear-snes').setAttribute('aria-pressed', String(side === 'snes'));
  $('hear-tg16').setAttribute('aria-pressed', String(side === 'tg16'));
  audio?.flush();
  status();
}

// ---------------------------------------------------------------- input

// ppu-cli/docs/pad.md's SNES keys. SNES bits: up down left right a b x y l r
// start select. TG16 bits: up down left right I II select run, with X = I
// (SNES A), Z = II (SNES B), Enter = Run, Shift = Select.
const KEYS = {
  ArrowUp: [0, 0], ArrowDown: [1, 1], ArrowLeft: [2, 2], ArrowRight: [3, 3],
  KeyX: [4, 4], KeyZ: [5, 5], KeyS: [6, null], KeyA: [7, null],
  KeyQ: [8, null], KeyW: [9, null], Enter: [10, 7], ShiftLeft: [11, 6], ShiftRight: [11, 6],
};
const held = new Set();
function updatePad() {
  padMask = { snes: 0, tg16: 0 };
  for (const code of held) {
    const [s, t] = KEYS[code];
    padMask.snes |= 1 << s;
    if (t !== null) padMask.tg16 |= 1 << t;
  }
}

// ---------------------------------------------------------------- ui

function fit() {
  const w = Math.max(256, $('tg16-screen').width);
  const avail = $('snes').querySelector('.stage').clientWidth;
  const availH = Math.max(224, window.innerHeight - 260);
  const scale = Math.max(1, Math.floor(Math.min(avail / w, availH / 224)));
  for (const id of ['snes-screen', 'tg16-screen']) {
    const c = $(id);
    c.style.width = `${c.width * scale}px`;
    c.style.height = `${c.height * scale}px`;
  }
}

function togglePause() {
  paused = !paused;
  acc = 0;
  if (paused) audio?.flush();
  $('pause').textContent = paused ? 'Resume' : 'Pause';
  $('step').disabled = !paused;
  status();
}

function buildPicker() {
  const sel = $('toy');
  for (const group of [...new Set(TOYS.map((t) => t.group))]) {
    const og = document.createElement('optgroup');
    og.label = group;
    for (const t of TOYS.filter((x) => x.group === group)) og.append(new Option(t.label, t.id));
    sel.append(og);
  }
}

function wireUi() {
  const sel = $('toy');
  sel.onchange = () => selectToy(sel.value);
  $('sound').onclick = () => toggleSound().catch((e) => msg(hear, `sound: ${errorText(e)}`, 'error'));
  $('hear-snes').onclick = () => setHear('snes');
  $('hear-tg16').onclick = () => setHear('tg16');
  $('pause').onclick = togglePause;
  $('step').onclick = step;
  $('restart').onclick = bootMachines;
  window.addEventListener('keydown', (e) => {
    if (e.target instanceof HTMLSelectElement || e.metaKey || e.ctrlKey || e.altKey) return;
    if (KEYS[e.code]) {
      e.preventDefault();
      held.add(e.code);
      updatePad();
    } else if (e.key === ' ') { e.preventDefault(); togglePause(); }
    else if (e.key === '.') step();
    else if (e.key === 'r') bootMachines();
  });
  window.addEventListener('keyup', (e) => {
    if (held.delete(e.code)) updatePad();
  });
  window.addEventListener('blur', () => { held.clear(); updatePad(); });
  window.addEventListener('resize', fit);
}

async function main() {
  wireUi();
  try {
    [tgWasm] = await Promise.all([initTg16(), initSnes()]);
  } catch (e) {
    $('status').textContent = `could not load the wasm modules (run ./build.sh first): ${errorText(e)}`;
    console.error(e);
    return;
  }
  try {
    const extra = await fetchJson('pkg/compare.json');
    for (const id of extra.tg16Only || []) {
      TOYS.push({ id, label: id, group: 'TG16 only', snesUrl: null, tg16Url: `pkg/${id}.tg16.json` });
    }
  } catch (e) {
    console.warn(`pkg/compare.json: ${errorText(e)}`);
  }
  buildPicker();
  fit();
  await selectToy(new URLSearchParams(location.search).get('toy') || TOYS[0].id);
  // For debugging and tests.
  window.compareViewer = {
    get frame() { return frameNo; },
    get snesFailed() { return !snes || snes.failed; },
    get tg16Failed() { return !tg || tg.failed; },
    select: selectToy,
    get snesCore() { return snes?.core; },
    get tg16Core() { return tg?.core; },
    toys: TOYS.map((t) => t.id),
  };
  requestAnimationFrame(tick);
}

main();
