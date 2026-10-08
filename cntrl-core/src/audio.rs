//! System audio → frequency bands for the audio-reactive canvas effects.
//!
//! Capture is a `parec` child process recording the default output's monitor
//! as raw PCM on its stdout; a reader thread turns each block into
//! [`BANDS`] smoothed levels. Nothing runs unless an audio effect is active.
//!
//! ponytail: shells out to `parec` (ships with pipewire-pulse / PulseAudio)
//! instead of linking PipeWire — no new build dependencies. Upgrade to the
//! `pipewire` crate if the extra process or the ~25 ms block latency matters.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

/// Frequency bands handed to effects, low to high.
pub const BANDS: usize = 16;
const RATE: f32 = 44_100.0;
/// Samples per analysis block (a power of two for the FFT): ~23 ms.
const BLOCK: usize = 1024;

/// What effects see: each band and the overall level, all in 0..=1.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AudioLevels {
    pub bands: [f32; BANDS],
    pub level: f32,
}

/// Turns PCM blocks into smoothed, auto-gained band levels.
pub struct Analyzer {
    levels: AudioLevels,
    /// Loudest band magnitude seen recently — the reference for "full".
    peak: f32,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self {
            levels: AudioLevels::default(),
            peak: PEAK_FLOOR,
        }
    }
}

/// Below this the input counts as silence, so quiet noise isn't amplified
/// into a full-height spectrum.
const PEAK_FLOOR: f32 = 0.5;

impl Analyzer {
    /// Feed one block of `BLOCK` samples in -1..=1; returns the new levels.
    pub fn push(&mut self, samples: &[f32]) -> AudioLevels {
        let mags = band_magnitudes(samples);
        let loudest = mags.iter().cloned().fold(0.0, f32::max);
        // Auto gain: follow the loudest recent band, forgetting slowly, so
        // both quiet and loud music fill the range.
        self.peak = (self.peak * 0.995).max(loudest).max(PEAK_FLOOR);
        let mut sum = 0.0;
        for (level, mag) in self.levels.bands.iter_mut().zip(mags) {
            let target = (mag / self.peak).sqrt().min(1.0);
            // Jump up at once, fall gradually — reads as a beat, not a flicker.
            *level = if target > *level { target } else { *level * 0.82 + target * 0.18 };
            sum += *level;
        }
        self.levels.level = sum / BANDS as f32;
        self.levels
    }
}

/// Spectrum magnitude summed into `BANDS` log-spaced bands, 40 Hz – 14 kHz.
fn band_magnitudes(samples: &[f32]) -> [f32; BANDS] {
    let n = BLOCK.min(samples.len());
    let mut re = [0.0f32; BLOCK];
    let mut im = [0.0f32; BLOCK];
    for (i, s) in samples[..n].iter().enumerate() {
        // Hann window: without it one tone smears into every band.
        let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / BLOCK as f32).cos();
        re[i] = s * w;
    }
    fft(&mut re, &mut im);
    let (lo, hi) = (40.0f32, 14_000.0f32);
    let edge = |b: usize| lo * (hi / lo).powf(b as f32 / BANDS as f32);
    let hz_per_bin = RATE / BLOCK as f32;
    std::array::from_fn(|b| {
        let first = ((edge(b) / hz_per_bin) as usize).max(1);
        let last = ((edge(b + 1) / hz_per_bin) as usize).clamp(first + 1, BLOCK / 2);
        (first..last)
            .map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt())
            .fold(0.0, f32::max)
    })
}

/// In-place radix-2 FFT (`re.len()` must be a power of two).
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    // Bit-reversal reordering.
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let step = -std::f32::consts::TAU / len as f32;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (step * k as f32).sin_cos();
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * c - im[b] * s, re[b] * s + im[b] * c);
                (re[b], im[b]) = (re[a] - tr, im[a] - ti);
                (re[a], im[a]) = (re[a] + tr, im[a] + ti);
            }
        }
        len <<= 1;
    }
}

/// A running capture. Dropping it stops `parec`.
pub struct AudioCapture {
    child: Child,
    levels: Arc<Mutex<AudioLevels>>,
}

impl AudioCapture {
    pub fn start() -> Result<Self, String> {
        let mut child = Command::new("parec")
            .args([
                "--device=@DEFAULT_MONITOR@",
                "--format=s16le",
                "--rate=44100",
                "--channels=1",
                "--latency-msec=20",
                "--raw",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("audio capture needs `parec` (pipewire-pulse): {e}"))?;
        let mut pcm = child.stdout.take().expect("piped stdout");
        let levels = Arc::new(Mutex::new(AudioLevels::default()));
        let shared = Arc::clone(&levels);
        std::thread::Builder::new()
            .name("cntrl-audio".to_string())
            .spawn(move || {
                let mut analyzer = Analyzer::default();
                let mut bytes = [0u8; BLOCK * 2];
                // Ends when parec exits (killed on drop, or the audio server went away).
                while pcm.read_exact(&mut bytes).is_ok() {
                    let samples: Vec<f32> = bytes
                        .chunks_exact(2)
                        .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
                        .collect();
                    *shared.lock().expect("audio levels") = analyzer.push(&samples);
                }
                // No more audio: fall silent rather than freeze on the last block.
                *shared.lock().expect("audio levels") = AudioLevels::default();
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { child, levels })
    }

    pub fn levels(&self) -> AudioLevels {
        *self.levels.lock().expect("audio levels")
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f32, amplitude: f32) -> Vec<f32> {
        (0..BLOCK)
            .map(|i| amplitude * (std::f32::consts::TAU * hz * i as f32 / RATE).sin())
            .collect()
    }

    fn loudest(levels: &AudioLevels) -> usize {
        (0..BANDS)
            .max_by(|&a, &b| levels.bands[a].total_cmp(&levels.bands[b]))
            .unwrap()
    }

    #[test]
    fn tones_land_in_rising_bands() {
        let mut last = 0;
        for hz in [80.0, 440.0, 1000.0, 5000.0, 11_000.0] {
            let levels = Analyzer::default().push(&sine(hz, 0.8));
            let band = loudest(&levels);
            assert!(levels.bands[band] > 0.9, "{hz} Hz: weak peak {}", levels.bands[band]);
            assert!(band > last || hz == 80.0, "{hz} Hz landed in band {band}, not above {last}");
            last = band;
        }
        // 1 kHz sits mid-spectrum, and far-away bands stay quiet.
        let levels = Analyzer::default().push(&sine(1000.0, 0.8));
        assert!((6..=10).contains(&loudest(&levels)));
        assert!(levels.bands[0] < 0.2 && levels.bands[BANDS - 1] < 0.2);
    }

    #[test]
    fn silence_and_faint_noise_stay_dark_and_levels_decay() {
        let mut a = Analyzer::default();
        assert_eq!(a.push(&vec![0.0; BLOCK]).level, 0.0);
        // Hiss far below the floor must not be auto-gained into a full display.
        assert!(a.push(&sine(3000.0, 0.0005)).level < 0.05);

        a.push(&sine(440.0, 0.8));
        let after_one = a.push(&vec![0.0; BLOCK]).level;
        let mut after_many = after_one;
        for _ in 0..40 {
            after_many = a.push(&vec![0.0; BLOCK]).level;
        }
        assert!(after_one > 0.0 && after_many < after_one * 0.01);
    }
}
