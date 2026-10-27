// TG16 toy viewer: loads a packed toy into the tg16-core wasm build and plays
// it at 60 Hz with sound. No bundler; run ./build.sh first to make pkg/.
import init, { Tg16 } from './pkg/tg16_core.js';

const FPS = 60;
const STEP_MS = 1000 / FPS;
const SAMPLE_RATE = 44100;

const $ = (id) => document.getElementById(id);
const canvas = $('screen');
const ctx = canvas.getContext('2d');

let wasm; // init() exports (for memory)
let tg; // Tg16 instance
let toy = null; // { name, files: [[name, src]], images: [{name,width,height,rgba}] }
let frameNo = 0;
let paused = false;
let failed = false;
let acc = 0;
let last = 0;
let audio = null; // { push(Int16Array), ctx }
let fpsFrames = 0;
let fpsT = 0;
let fps = 0;

// ---------------------------------------------------------------- messages

function log(text, kind = 'info') {
  const li = document.createElement('li');
  li.className = kind;
  li.textContent = text;
  const ul = $('log');
  ul.prepend(li);
  while (ul.children.length > 50) ul.lastChild.remove();
  // Toy errors are the author's, not the page's: warn rather than error.
  if (kind === 'error') console.warn(text);
}

function errorText(e) {
  if (e && typeof e === 'object' && 'message' in e) return e.message;
  return String(e);
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

const stem = (file) => file.replace(/^.*\//, '').replace(/\.[^.]*$/, '');

/** Lua load order: names sorted, main.lua last. */
function orderFiles(entries) {
  return entries.sort(([a], [b]) => {
    if (a === 'main.lua') return 1;
    if (b === 'main.lua') return -1;
    return a < b ? -1 : a > b ? 1 : 0;
  });
}

async function toyFromPack(pack, fallbackName) {
  if (pack.format !== 'ppu.toys/tg16') {
    throw new Error(`not a TG16 pack (format is ${JSON.stringify(pack.format)}, want "ppu.toys/tg16")`);
  }
  if (pack.version !== 1) log(`pack version ${pack.version}; this viewer knows version 1`, 'warn');
  const files = orderFiles(Object.entries(pack.files || {}));
  if (!files.length) throw new Error('pack has no Lua files');
  const images = [];
  for (const [name, b64] of Object.entries(pack.assets || {})) {
    if (!/\.png$/i.test(name)) {
      log(`skipping asset ${name}: only PNG is supported`, 'warn');
      continue;
    }
    images.push(await decodeImage(stem(name), b64ToBytes(b64)));
  }
  return { name: pack.name || fallbackName, files, images };
}

/** A pack, a single .lua, or a set of .lua + .png files. */
async function toyFromFiles(fileList) {
  const list = [...fileList];
  const json = list.find((f) => /\.json$/i.test(f.name));
  if (json) {
    return toyFromPack(JSON.parse(await json.text()), json.name.replace(/\.tg16\.json$|\.json$/i, ''));
  }
  const lua = list.filter((f) => /\.lua$/i.test(f.name));
  if (!lua.length) throw new Error('drop a .tg16.json pack or at least one .lua file');
  const files = orderFiles(await Promise.all(lua.map(async (f) => [f.name, await f.text()])));
  const images = [];
  for (const f of list.filter((f) => /\.png$/i.test(f.name))) {
    images.push(await decodeImage(stem(f.name), new Uint8Array(await f.arrayBuffer())));
  }
  const main = lua.find((f) => f.name === 'main.lua') || lua[0];
  return { name: lua.length === 1 ? stem(main.name) : stem(main.name) + ` (+${lua.length - 1})`, files, images };
}

function start(next) {
  toy = next;
  $('toy-name').textContent = toy.name;
  tg.clear_images();
  for (const im of toy.images) tg.add_image(im.name, im.width, im.height, im.rgba);
  restart();
  const extra = toy.images.length ? `, ${toy.images.length} image(s)` : '';
  log(`loaded ${toy.name}: ${toy.files.map((f) => f[0]).join(', ')}${extra}`);
}

function restart() {
  if (!toy) return;
  failed = false;
  frameNo = 0;
  acc = 0;
  audio?.flush();
  try {
    tg.load(JSON.stringify(toy.files));
  } catch (e) {
    fail(e);
    return;
  }
  showWarnings();
  // Draw frame 0 right away so a paused restart shows something.
  if (paused) step();
}

function fail(e) {
  failed = true;
  log(errorText(e), 'error');
  updateButtons();
}

let warned = new Set();
function showWarnings() {
  let list = [];
  try {
    list = JSON.parse(tg.warnings_json());
  } catch {}
  for (const w of list) {
    if (!warned.has(w)) {
      warned.add(w);
      log(w, 'warn');
    }
  }
}

async function loadDemo() {
  try {
    const res = await fetch('pkg/demo.tg16.json');
    if (!res.ok) throw new Error(`demo: HTTP ${res.status}`);
    start(await toyFromPack(await res.json(), 'demo'));
  } catch (e) {
    log(`could not load the demo: ${errorText(e)}`, 'error');
  }
}

async function openFiles(files) {
  try {
    warned = new Set();
    start(await toyFromFiles(files));
  } catch (e) {
    log(errorText(e), 'error');
  }
}

// ---------------------------------------------------------------- running

// Joypad: bits up, down, left, right, I, II, select, run. Keys follow the
// SNES viewer's (ppu-cli/docs/pad.md): X = I (SNES A), Z = II (SNES B),
// Enter = Run (Start), Shift = Select.
const PAD_KEYS = {
  ArrowUp: 0, ArrowDown: 1, ArrowLeft: 2, ArrowRight: 3,
  KeyX: 4, KeyZ: 5, ShiftLeft: 6, ShiftRight: 6, Enter: 7,
};
const held = new Set();
function padMask() {
  let m = 0;
  for (const code of held) m |= 1 << PAD_KEYS[code];
  return m;
}

function runFrame() {
  try {
    tg.set_pad(padMask());
    tg.frame(frameNo / FPS, frameNo);
  } catch (e) {
    fail(e);
    return false;
  }
  frameNo++;
  if (audio) audio.push(tg.audio());
  return true;
}

function draw() {
  const w = tg.width();
  const h = tg.height();
  if (canvas.width !== w || canvas.height !== h) {
    canvas.width = w;
    canvas.height = h;
    fit();
  }
  // Zero-copy view of the frame in wasm memory; rebuilt every frame because
  // the buffer moves when the frame is replaced or memory grows.
  const px = new Uint8ClampedArray(wasm.memory.buffer, tg.pixels_ptr(), tg.pixels_len());
  ctx.putImageData(new ImageData(px, w, h), 0, 0);
}

let budgetTick = 0;
function showBudget(force) {
  if (!force && ++budgetTick % 6) return;
  let b;
  try {
    b = JSON.parse(tg.budget_json());
  } catch {
    return;
  }
  const bar = (id, frac) => {
    const el = $(id);
    el.style.width = `${Math.min(1, frac) * 100}%`;
    el.className = frac > 1 ? 'bad' : frac > 0.8 ? 'warn' : '';
  };
  $('m-cpu').textContent = `${(b.cpu * 100).toFixed(1)}%`;
  bar('b-cpu', b.cpu);
  $('m-raster').textContent = `${b.rasterLines}`;
  $('m-soft').textContent = b.software ? `${b.software} (${(b.software / b.cyclesPerFrame * 100).toFixed(1)}%)` : '0';
  $('m-sprites').textContent = `${b.sprites} / 64`;
  bar('b-sprites', b.sprites / 64);
  $('m-units').textContent = `${b.peakLineUnits} / 16${b.droppedLines ? ` (dropped on ${b.droppedLines} lines)` : ''}`;
  bar('b-units', b.peakLineUnits / 16 + (b.droppedLines ? 0.01 : 0));
  $('m-vram').textContent = `${b.vramWords}`;
  const ul = $('problems');
  while (ul.childElementCount > b.problems.length) ul.lastChild.remove();
  while (ul.childElementCount < b.problems.length) ul.append(document.createElement('li'));
  b.problems.forEach((p, i) => (ul.children[i].textContent = p));
}

function status() {
  const state = failed ? 'stopped (error)' : paused ? 'paused' : 'running';
  const snd = audio ? `sound ${audio.ctx.sampleRate} Hz` : 'sound off';
  $('status').textContent = `frame ${frameNo} · t ${(frameNo / FPS).toFixed(2)} s · ${state} · ${fps} fps · ${tg.width()}×${tg.height()} · ${snd}`;
}

function tick(now) {
  requestAnimationFrame(tick);
  if (!last) last = now;
  let dt = now - last;
  last = now;
  if (paused || failed || !toy) return;
  // Fixed step: catch up at most a few frames after a stall (tab hidden etc).
  acc += Math.min(dt, 100);
  let ran = 0;
  while (acc >= STEP_MS && ran < 4) {
    if (!runFrame()) break;
    acc -= STEP_MS;
    ran++;
  }
  if (ran >= 4) acc = 0;
  if (ran) {
    draw();
    showBudget(false);
    fpsFrames += ran;
  }
  if (now - fpsT >= 500) {
    fps = Math.round((fpsFrames * 1000) / (now - fpsT));
    fpsFrames = 0;
    fpsT = now;
    status();
    showWarnings();
  }
}

function step() {
  if (failed || !toy) return;
  if (runFrame()) {
    draw();
    showBudget(true);
    showWarnings();
  }
  status();
}

// ---------------------------------------------------------------- audio

const WORKLET = `
class Tg16Out extends AudioWorkletProcessor {
  constructor(opts) {
    super();
    this.ratio = opts.processorOptions.ratio; // source samples per output sample
    this.q = []; this.cur = null; this.pos = 0; this.queued = 0;
    this.port.onmessage = (e) => {
      if (e.data === 'flush') { this.q = []; this.cur = null; this.queued = 0; return; }
      const d = e.data; // Float32Array, interleaved stereo
      // Keep latency bounded: drop the backlog past ~150 ms.
      if (this.queued > 44100 * 0.15) { this.q = []; this.queued = 0; }
      this.q.push(d); this.queued += d.length / 2;
    };
  }
  process(_in, outputs) {
    const out = outputs[0]; const L = out[0]; const R = out[1] || out[0];
    for (let i = 0; i < L.length; i++) {
      if (!this.cur || this.pos >= this.cur.length / 2) {
        if (this.cur) { this.queued -= this.cur.length / 2; this.pos -= this.cur.length / 2; }
        this.cur = this.q.shift() || null;
        if (!this.cur) { this.pos = 0; L[i] = 0; R[i] = 0; continue; }
        if (this.pos < 0) this.pos = 0;
      }
      const j = Math.floor(this.pos) * 2;
      L[i] = this.cur[j]; R[i] = this.cur[j + 1];
      this.pos += this.ratio;
    }
    return true;
  }
}
registerProcessor('tg16-out', Tg16Out);
`;

function toFloat(pcm) {
  const f = new Float32Array(pcm.length);
  for (let i = 0; i < pcm.length; i++) f[i] = pcm[i] / 32768;
  return f;
}

async function startAudio() {
  if (audio) {
    // Toggle off.
    await audio.ctx.close();
    audio = null;
    $('sound').textContent = '▶ Sound on';
    $('sound').classList.add('primary');
    status();
    return;
  }
  let ac;
  try {
    ac = new AudioContext({ sampleRate: SAMPLE_RATE, latencyHint: 'interactive' });
  } catch {
    ac = new AudioContext();
  }
  const ratio = SAMPLE_RATE / ac.sampleRate;
  if (ac.audioWorklet) {
    const url = URL.createObjectURL(new Blob([WORKLET], { type: 'text/javascript' }));
    await ac.audioWorklet.addModule(url);
    URL.revokeObjectURL(url);
    const node = new AudioWorkletNode(ac, 'tg16-out', {
      numberOfInputs: 0,
      outputChannelCount: [2],
      processorOptions: { ratio },
    });
    node.connect(ac.destination);
    audio = {
      ctx: ac,
      push: (pcm) => {
        const f = toFloat(pcm);
        node.port.postMessage(f, [f.buffer]);
      },
      flush: () => node.port.postMessage('flush'),
    };
  } else {
    // ScriptProcessor fallback (older browsers, insecure origins).
    const sp = ac.createScriptProcessor(1024, 0, 2);
    let q = [];
    let cur = null;
    let pos = 0;
    sp.onaudioprocess = (e) => {
      const L = e.outputBuffer.getChannelData(0);
      const R = e.outputBuffer.getChannelData(1);
      for (let i = 0; i < L.length; i++) {
        if (!cur || pos >= cur.length / 2) {
          if (cur) pos -= cur.length / 2;
          cur = q.shift() || null;
          if (!cur) { pos = 0; L[i] = R[i] = 0; continue; }
        }
        const j = Math.floor(pos) * 2;
        L[i] = cur[j];
        R[i] = cur[j + 1];
        pos += ratio;
      }
    };
    sp.connect(ac.destination);
    audio = {
      ctx: ac,
      push: (pcm) => {
        if (q.length > 9) q = [];
        q.push(toFloat(pcm));
      },
      flush: () => { q = []; cur = null; },
    };
  }
  await ac.resume();
  $('sound').textContent = '■ Sound off';
  $('sound').classList.remove('primary');
  status();
}

// ---------------------------------------------------------------- ui

function fit() {
  const stage = $('stage');
  const w = canvas.width;
  const h = canvas.height;
  const availW = stage.clientWidth;
  const availH = Math.max(240, window.innerHeight - stage.getBoundingClientRect().top - 60);
  const scale = Math.max(1, Math.floor(Math.min(availW / w, availH / h)));
  canvas.style.width = `${w * scale}px`;
  canvas.style.height = `${h * scale}px`;
  stage.style.height = `${h * scale}px`;
}

function updateButtons() {
  $('pause').textContent = paused ? 'Resume' : 'Pause';
  $('step').disabled = !paused || failed;
}

function togglePause() {
  paused = !paused;
  acc = 0;
  if (paused) audio?.flush();
  updateButtons();
  status();
}

function wireUi() {
  $('sound').onclick = () => startAudio().catch((e) => log(`sound: ${errorText(e)}`, 'error'));
  $('pause').onclick = togglePause;
  $('step').onclick = step;
  $('restart').onclick = () => { restart(); status(); };
  $('demo').onclick = () => { warned = new Set(); loadDemo(); };
  $('file').onchange = (e) => { if (e.target.files.length) openFiles(e.target.files); e.target.value = ''; };
  let depth = 0;
  window.addEventListener('dragenter', (e) => { e.preventDefault(); depth++; document.body.classList.add('dragging'); });
  window.addEventListener('dragleave', () => { if (--depth <= 0) { depth = 0; document.body.classList.remove('dragging'); } });
  window.addEventListener('dragover', (e) => e.preventDefault());
  window.addEventListener('drop', (e) => {
    e.preventDefault();
    depth = 0;
    document.body.classList.remove('dragging');
    if (e.dataTransfer.files.length) openFiles(e.dataTransfer.files);
  });
  window.addEventListener('keydown', (e) => {
    if (e.target instanceof HTMLInputElement || e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.code in PAD_KEYS) { e.preventDefault(); held.add(e.code); }
    else if (e.key === ' ') { e.preventDefault(); togglePause(); }
    else if (e.key === '.') { if (paused) step(); }
    else if (e.key === 'r') { restart(); status(); }
  });
  window.addEventListener('keyup', (e) => held.delete(e.code));
  window.addEventListener('blur', () => held.clear());
  window.addEventListener('resize', fit);
}

async function main() {
  wireUi();
  try {
    wasm = await init();
  } catch (e) {
    log(`could not load pkg/tg16_core_bg.wasm. Run ./build.sh first. (${errorText(e)})`, 'error');
    return;
  }
  tg = new Tg16();
  fit();
  updateButtons();
  // ?toy=path/to/pack.tg16.json loads that instead of the demo.
  const url = new URLSearchParams(location.search).get('toy');
  if (url) {
    try {
      const res = await fetch(url);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      start(await toyFromPack(await res.json(), stem(url)));
    } catch (e) {
      log(`could not load ${url}: ${errorText(e)}`, 'error');
      await loadDemo();
    }
  } else {
    await loadDemo();
  }
  // Expose for debugging and tests.
  window.tg16Viewer = { get frame() { return frameNo; }, get failed() { return failed; }, tg };
  requestAnimationFrame(tick);
}

main();
