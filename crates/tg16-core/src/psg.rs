//! HuC6280 PSG: six wavetable channels.
//!
//! Each channel plays a 32-step waveform of 5-bit samples. Its 12-bit period
//! `F` sets the pitch: the wave advances one step every `F` ticks of the
//! 3.58 MHz clock, so a full cycle is `3_579_545 / (32 * F)` Hz. Channels 5
//! and 6 (indices 4 and 5) can switch to noise. Channel 2 can act as an LFO
//! that bends channel 1's period. Any channel can switch to DDA mode, where
//! the CPU writes samples straight to the output (how TG16 games played
//! drums and voices).
//!
//! Volume is a chain of attenuators in decibels: channel volume (5 bits,
//! 1.5 dB per step), channel left/right balance (4 bits, 3 dB per step) and
//! the master balance (4 bits, 3 dB per step).

/// PSG master clock.
pub const PSG_CLOCK: f64 = 3_579_545.0;
/// Output sample rate.
pub const SAMPLE_RATE: u32 = 44_100;
/// Output samples per 60 Hz frame (rounded; the engine carries the remainder).
pub const SAMPLES_PER_FRAME: f64 = SAMPLE_RATE as f64 / 60.0;

pub const CHANNELS: usize = 6;

/// One channel's registers.
#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    /// 12-bit period. 0 behaves like 4096 on hardware; here it is silent.
    pub period: u16,
    pub on: bool,
    /// 0..31.
    pub volume: u8,
    /// 0..15 each.
    pub left: u8,
    pub right: u8,
    /// 32 samples, 0..31.
    pub wave: [u8; 32],
    /// Noise mode (channels 5 and 6 only) and its 5-bit frequency.
    pub noise: bool,
    pub noise_freq: u8,
    /// DDA mode: the channel outputs `dda_level` instead of its wave.
    pub dda: bool,
    pub dda_level: u8,
}

impl Default for Channel {
    fn default() -> Self {
        Channel {
            period: 0,
            on: false,
            volume: 31,
            left: 15,
            right: 15,
            wave: [16; 32],
            noise: false,
            noise_freq: 0,
            dda: false,
            dda_level: 16,
        }
    }
}

/// LFO control: channel 2's wave bends channel 1's period.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Lfo {
    /// Channel 2 advances once every `freq * period2` ticks (0 = 256).
    pub freq: u8,
    /// 0 off, 1..3 scale the bend by 1, 16 or 256.
    pub mode: u8,
}

/// The sound chip.
#[derive(Clone, Debug)]
pub struct Psg {
    pub channels: [Channel; CHANNELS],
    pub master_left: u8,
    pub master_right: u8,
    pub lfo: Lfo,
    // Running state.
    pos: [u8; CHANNELS],
    acc: [f64; CHANNELS],
    lfsr: [u32; CHANNELS],
    noise_out: [bool; CHANNELS],
}

impl Default for Psg {
    fn default() -> Self {
        Psg {
            channels: Default::default(),
            master_left: 15,
            master_right: 15,
            lfo: Lfo::default(),
            pos: [0; CHANNELS],
            acc: [0.0; CHANNELS],
            lfsr: [1; CHANNELS],
            noise_out: [false; CHANNELS],
        }
    }
}

/// Linear gain for an attenuation in decibels.
fn gain(db: f64) -> f64 {
    10f64.powf(-db / 20.0)
}

/// Period for a frequency in Hz, rounded and clamped to 12 bits.
pub fn period_for_hz(hz: f64) -> u16 {
    if hz <= 0.0 {
        return 0;
    }
    (PSG_CLOCK / (32.0 * hz)).round().clamp(1.0, 4095.0) as u16
}

/// Frequency in Hz a period plays.
pub fn hz_for_period(period: u16) -> f64 {
    if period == 0 {
        0.0
    } else {
        PSG_CLOCK / (32.0 * period as f64)
    }
}

/// Built-in waveforms, 32 steps of 0..31.
pub fn preset_wave(name: &str) -> Option<[u8; 32]> {
    let f: fn(usize) -> f64 = match name {
        "sine" => |i| ((i as f64 / 32.0 * std::f64::consts::TAU).sin() + 1.0) * 15.5,
        "square" => |i| if i < 16 { 31.0 } else { 0.0 },
        "pulse25" => |i| if i < 8 { 31.0 } else { 0.0 },
        "pulse12" => |i| if i < 4 { 31.0 } else { 0.0 },
        "saw" => |i| i as f64,
        "triangle" => |i| {
            if i < 16 {
                i as f64 * 2.0
            } else {
                (31 - i) as f64 * 2.0
            }
        },
        _ => return None,
    };
    Some(std::array::from_fn(|i| f(i).round().clamp(0.0, 31.0) as u8))
}

impl Psg {
    /// Restart a channel's wave at step 0, as writing its waveform does.
    pub fn reset_phase(&mut self, ch: usize) {
        self.pos[ch] = 0;
        self.acc[ch] = 0.0;
    }

    /// Render `n` stereo frames, appending interleaved L/R i16 to `out`.
    #[allow(clippy::needless_range_loop)] // ch indexes several per-channel arrays
    pub fn render(&mut self, n: usize, out: &mut Vec<i16>) {
        let ticks = PSG_CLOCK / SAMPLE_RATE as f64;
        let master_l = (15 - self.master_left.min(15)) as f64 * 3.0;
        let master_r = (15 - self.master_right.min(15)) as f64 * 3.0;
        // Precompute per-channel gains.
        let gains: Vec<(f64, f64)> = self
            .channels
            .iter()
            .map(|c| {
                let v = (31 - c.volume.min(31)) as f64 * 1.5;
                let l = v + (15 - c.left.min(15)) as f64 * 3.0 + master_l;
                let r = v + (15 - c.right.min(15)) as f64 * 3.0 + master_r;
                let mute = |b: u8, m: u8| b == 0 || m == 0 || c.volume == 0;
                (
                    if mute(c.left, self.master_left) {
                        0.0
                    } else {
                        gain(l)
                    },
                    if mute(c.right, self.master_right) {
                        0.0
                    } else {
                        gain(r)
                    },
                )
            })
            .collect();
        let lfo_on = self.lfo.mode != 0;
        out.reserve(n * 2);
        for _ in 0..n {
            let (mut l, mut r) = (0.0, 0.0);
            for ch in 0..CHANNELS {
                let c = &self.channels[ch];
                if !c.on {
                    continue;
                }
                let level = if c.dda {
                    c.dda_level
                } else if c.noise && ch >= 4 {
                    // Noise clock: one LFSR step every (31 - freq) * 64 ticks.
                    let period = ((31 - (c.noise_freq & 31)) as f64 * 64.0).max(64.0);
                    self.acc[ch] += ticks;
                    while self.acc[ch] >= period {
                        self.acc[ch] -= period;
                        let s = self.lfsr[ch];
                        let bit = (s ^ (s >> 1)) & 1;
                        self.lfsr[ch] = (s >> 1) | (bit << 14);
                        self.noise_out[ch] = s & 1 != 0;
                    }
                    if self.noise_out[ch] {
                        31
                    } else {
                        0
                    }
                } else {
                    let mut period = c.period as f64;
                    if ch == 0 && lfo_on {
                        let bend = self.channels[1].wave[self.pos[1] as usize] as f64 - 16.0;
                        let scale = [0.0, 1.0, 16.0, 256.0][self.lfo.mode as usize & 3];
                        period = (period + bend * scale).clamp(1.0, 4095.0);
                    }
                    if ch == 1 && lfo_on {
                        // Channel 2 runs at LFO speed and is not heard.
                        let f = if self.lfo.freq == 0 {
                            256.0
                        } else {
                            self.lfo.freq as f64
                        };
                        let p = (c.period.max(1) as f64) * f;
                        self.acc[ch] += ticks;
                        while self.acc[ch] >= p {
                            self.acc[ch] -= p;
                            self.pos[ch] = (self.pos[ch] + 1) & 31;
                        }
                        continue;
                    }
                    if c.period == 0 {
                        continue;
                    }
                    self.acc[ch] += ticks;
                    while self.acc[ch] >= period {
                        self.acc[ch] -= period;
                        self.pos[ch] = (self.pos[ch] + 1) & 31;
                    }
                    c.wave[self.pos[ch] as usize]
                };
                let s = level as f64 - 15.5;
                l += s * gains[ch].0;
                r += s * gains[ch].1;
            }
            // Scaled so four full-volume channels reach full scale; music
            // rarely has all six loud at once, and the clamp catches it.
            let k = 32767.0 / (15.5 * 4.0);
            out.push((l * k).round().clamp(-32768.0, 32767.0) as i16);
            out.push((r * k).round().clamp(-32768.0, 32767.0) as i16);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(period: u16) -> Psg {
        let mut p = Psg::default();
        p.channels[0] = Channel {
            period,
            on: true,
            wave: preset_wave("square").unwrap(),
            ..Channel::default()
        };
        p
    }

    fn rising_edges(left: &[i16]) -> usize {
        left.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count()
    }

    #[test]
    fn period_maps_to_pitch() {
        let period = period_for_hz(440.0);
        assert_eq!(period, 254);
        let mut p = tone(period);
        let mut out = Vec::new();
        p.render(SAMPLE_RATE as usize, &mut out);
        let left: Vec<i16> = out.iter().step_by(2).copied().collect();
        let edges = rising_edges(&left) as f64;
        assert!((edges - hz_for_period(period)).abs() <= 2.0, "{edges} Hz");
    }

    #[test]
    fn off_channel_is_silent() {
        let mut p = tone(254);
        p.channels[0].on = false;
        let mut out = Vec::new();
        p.render(1000, &mut out);
        assert!(out.iter().all(|&s| s == 0));
    }

    #[test]
    fn volume_steps_are_1_5_db() {
        let peak = |vol: u8| {
            let mut p = tone(254);
            p.channels[0].volume = vol;
            let mut out = Vec::new();
            p.render(2000, &mut out);
            out.iter().map(|s| s.unsigned_abs()).max().unwrap() as f64
        };
        let ratio = peak(31) / peak(27); // 4 steps = 6 dB = about 2x
        assert!((ratio - 2.0).abs() < 0.05, "{ratio}");
    }

    #[test]
    fn balance_pans() {
        let mut p = tone(254);
        p.channels[0].right = 0;
        let mut out = Vec::new();
        p.render(1000, &mut out);
        assert!(out.iter().step_by(2).any(|&s| s != 0));
        assert!(out.iter().skip(1).step_by(2).all(|&s| s == 0));
    }

    #[test]
    fn noise_only_on_channels_5_and_6() {
        let mut p = Psg::default();
        for ch in [0, 4] {
            p.channels[ch] = Channel {
                on: true,
                noise: true,
                noise_freq: 28,
                period: 0,
                ..Channel::default()
            };
        }
        let mut out = Vec::new();
        p.render(4000, &mut out);
        // Channel 0 ignores the noise bit (and has period 0, so it is
        // silent); channel 4 makes noise.
        let distinct: std::collections::HashSet<_> = out.iter().collect();
        assert!(distinct.len() >= 2);
        assert!(out.iter().any(|&s| s > 0) && out.iter().any(|&s| s < 0));
    }

    #[test]
    fn dda_outputs_its_level() {
        let mut p = Psg::default();
        p.channels[2] = Channel {
            on: true,
            dda: true,
            dda_level: 31,
            ..Channel::default()
        };
        let mut out = Vec::new();
        p.render(10, &mut out);
        assert!(out.iter().all(|&s| s == out[0] && s > 0));
    }

    #[test]
    fn lfo_bends_channel_1_and_mutes_channel_2() {
        let mut p = tone(254);
        p.channels[1] = Channel {
            period: 100,
            on: true,
            wave: preset_wave("square").unwrap(),
            ..Channel::default()
        };
        p.lfo = Lfo { freq: 200, mode: 2 };
        let mut out = Vec::new();
        p.render(SAMPLE_RATE as usize, &mut out);
        let left: Vec<i16> = out.iter().step_by(2).copied().collect();
        let edges = rising_edges(&left) as f64;
        // The bend is +/- 16 * 16 ticks around 254, so the average pitch moves.
        assert!((edges - 440.0).abs() > 20.0, "{edges} Hz");
    }

    #[test]
    fn presets_span_the_range() {
        for name in ["sine", "square", "saw", "triangle", "pulse25", "pulse12"] {
            let w = preset_wave(name).unwrap();
            assert!(w.iter().all(|&s| s <= 31));
            assert!(w.contains(&0) || name == "sine" || name == "triangle");
        }
        assert!(preset_wave("nope").is_none());
    }
}
