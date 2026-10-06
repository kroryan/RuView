//! Multi-subcarrier spectral estimator for breathing and heart rate.
//!
//! The legacy detector collapsed every CSI frame to one scalar (mean amplitude
//! or cross-subcarrier phase variance) and picked the highest FFT bin. That
//! discards most of the physical information (different subcarriers see the
//! chest motion with different gain and sign, so averaging them can cancel the
//! very signal being measured), assumes perfectly uniform frame arrival, and
//! reports a "rate" for any noise peak.
//!
//! This module instead:
//!
//! 1. keeps a **timestamped** window of per-subcarrier amplitude and
//!    *sanitised* phase (the per-frame linear phase slope/offset produced by
//!    sampling-time and carrier-frequency offsets is removed),
//! 2. resamples the window onto a uniform grid (UDP jitter and short bursts no
//!    longer smear the spectrum; a gap discards the stale history),
//! 3. screens every subcarrier by the spectral peak-to-floor ratio inside the
//!    physiological band, keeps the most periodic ones and **incoherently sums
//!    their noise-normalised power spectra**,
//! 4. excludes breathing harmonics from the heart-rate search, and
//! 5. reports a rate only if the combined peak is separated from the in-band
//!    median floor by more than the level expected from noise alone for that
//!    number of averaged subcarriers, scaled by how many of the selected
//!    subcarriers independently agree on the same frequency.
//!
//! All results are **SYNTHETIC-validated** by the unit tests in this module
//! (known sinusoids, jitter, harmonics and Monte-Carlo noise). They are not a
//! claim of clinical accuracy; heart rate from commodity Wi-Fi CSI remains
//! experimental and is still gated by explicit room calibration upstream.

use std::collections::VecDeque;
use std::f64::consts::PI;

use serde::Serialize;

/// Seconds of history analysed (also the breathing window).
pub const WINDOW_SECS: f64 = 30.0;
/// Minimum signal span before a breathing estimate is attempted.
pub const MIN_BREATHING_SECS: f64 = 12.0;
/// Minimum signal span before a heart-rate estimate is attempted.
pub const MIN_HEARTBEAT_SECS: f64 = 12.0;
/// A gap between consecutive frames longer than this invalidates the window.
pub const MAX_FRAME_GAP_SECS: f64 = 2.0;
/// Spectral refresh cadence in signal time.
pub const ESTIMATE_INTERVAL_SECS: f64 = 0.5;
/// Heart band reaches 2 Hz so the analysis rate must clear Nyquist with margin.
pub const MIN_HEART_ANALYSIS_RATE_HZ: f64 = 4.4;
pub const MIN_BREATH_ANALYSIS_RATE_HZ: f64 = 1.2;
pub const MAX_ANALYSIS_RATE_HZ: f64 = 50.0;
/// Half-width of the notch placed on every breathing harmonic.
pub const HARMONIC_TOLERANCE_HZ: f64 = 0.035;
pub const SELECTED_SUBCARRIERS_MAX: usize = 12;
/// Rates below this confidence are not reported at all.
pub const MIN_REPORTED_CONFIDENCE: f64 = 0.25;
/// Heart confidence is capped: commodity-CSI heart rate stays experimental.
pub const HEART_CONFIDENCE_CAP: f64 = 0.90;

pub const BREATHING_BAND_HZ: (f64, f64) = (0.1, 0.5);
pub const HEARTBEAT_BAND_HZ: (f64, f64) = (0.667, 2.0);

// ── Window ─────────────────────────────────────────────────────────────────

/// Timestamped per-subcarrier history.
#[derive(Debug, Default)]
pub struct SubcarrierWindow {
    /// Incremented every time history is discarded.
    generation: u64,
    n_sub: usize,
    times: VecDeque<f64>,
    amp: VecDeque<Vec<f32>>,
    /// Sanitised phase residual per frame; empty rows mean "no phase".
    phase: VecDeque<Vec<f32>>,
}

impl SubcarrierWindow {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn clear(&mut self) {
        self.generation += 1;
        self.n_sub = 0;
        self.times.clear();
        self.amp.clear();
        self.phase.clear();
    }

    pub fn len(&self) -> usize {
        self.times.len()
    }

    pub fn is_empty(&self) -> bool {
        self.times.is_empty()
    }

    pub fn span_secs(&self) -> f64 {
        match (self.times.front(), self.times.back()) {
            (Some(a), Some(b)) => (b - a).max(0.0),
            _ => 0.0,
        }
    }

    /// Append one frame. Returns `false` when the frame was rejected
    /// (non-monotonic timestamp or non-finite data).
    pub fn push(&mut self, t: f64, amplitude: &[f64], phase: &[f64]) -> bool {
        if !t.is_finite() || amplitude.is_empty() {
            return false;
        }
        if let Some(&last) = self.times.back() {
            if t <= last {
                return false;
            }
            if t - last > MAX_FRAME_GAP_SECS {
                // Samples across a long gap must never share one spectrum.
                self.clear();
            }
        }
        if self.n_sub != 0 && self.n_sub != amplitude.len() {
            // Grid change (e.g. HT20 -> HE-SU). Different bins are not comparable.
            self.clear();
        }
        if amplitude.iter().any(|a| !a.is_finite()) {
            return false;
        }
        self.n_sub = amplitude.len();
        self.times.push_back(t);
        self.amp.push_back(amplitude.iter().map(|&a| a as f32).collect());
        let mut sanitized = Vec::new();
        if phase.len() == amplitude.len() && phase.iter().all(|p| p.is_finite()) {
            sanitize_phase(phase, &mut sanitized);
        }
        self.phase.push_back(sanitized);
        while self.span_secs() > WINDOW_SECS {
            self.times.pop_front();
            self.amp.pop_front();
            self.phase.pop_front();
        }
        true
    }

    fn has_phase(&self) -> bool {
        !self.phase.is_empty() && self.phase.iter().all(|row| row.len() == self.n_sub)
    }
}

/// Remove the per-frame linear phase component (sampling-time offset slope and
/// common carrier offset) from wrapped CSI phase, returning the residual in
/// radians. Wrapped phase is first unwrapped along the subcarrier axis.
pub fn sanitize_phase(phase: &[f64], out: &mut Vec<f32>) {
    out.clear();
    let n = phase.len();
    if n < 4 {
        return;
    }
    let mut unwrapped = Vec::with_capacity(n);
    unwrapped.push(phase[0]);
    for i in 1..n {
        let mut d = phase[i] - phase[i - 1];
        d -= 2.0 * PI * (d / (2.0 * PI)).round();
        unwrapped.push(unwrapped[i - 1] + d);
    }
    let nf = n as f64;
    let mean_x = (nf - 1.0) / 2.0;
    let mean_y = unwrapped.iter().sum::<f64>() / nf;
    let (mut sxy, mut sxx) = (0.0, 0.0);
    for (i, y) in unwrapped.iter().enumerate() {
        let dx = i as f64 - mean_x;
        sxy += dx * (y - mean_y);
        sxx += dx * dx;
    }
    let slope = if sxx > 0.0 { sxy / sxx } else { 0.0 };
    out.extend(
        unwrapped
            .iter()
            .enumerate()
            .map(|(i, y)| (y - (mean_y + slope * (i as f64 - mean_x))) as f32),
    );
}

// ── FFT plan (precomputed twiddles) ─────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FftPlan {
    n: usize,
    cos: Vec<f64>,
    sin: Vec<f64>,
    rev: Vec<u32>,
}

impl FftPlan {
    pub fn new(n: usize) -> Self {
        assert!(n.is_power_of_two() && n >= 2);
        let bits = n.trailing_zeros();
        let rev = (0..n as u32)
            .map(|i| i.reverse_bits() >> (32 - bits))
            .collect();
        let half = n / 2;
        let cos = (0..half).map(|j| (2.0 * PI * j as f64 / n as f64).cos()).collect();
        let sin = (0..half).map(|j| (2.0 * PI * j as f64 / n as f64).sin()).collect();
        Self { n, cos, sin, rev }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    /// Power spectrum (`|X|^2`) for bins `0..=n/2` of the zero-padded input.
    pub fn power(&self, input: &[f64], re: &mut Vec<f64>, im: &mut Vec<f64>) -> Vec<f64> {
        let n = self.n;
        re.clear();
        re.resize(n, 0.0);
        im.clear();
        im.resize(n, 0.0);
        for (i, &x) in input.iter().take(n).enumerate() {
            re[self.rev[i] as usize] = x;
        }
        let mut size = 2;
        while size <= n {
            let half = size / 2;
            let step = n / size;
            for start in (0..n).step_by(size) {
                for k in 0..half {
                    let wr = self.cos[k * step];
                    let wi = -self.sin[k * step];
                    let i = start + k;
                    let j = i + half;
                    let tr = wr * re[j] - wi * im[j];
                    let ti = wr * im[j] + wi * re[j];
                    re[j] = re[i] - tr;
                    im[j] = im[i] - ti;
                    re[i] += tr;
                    im[i] += ti;
                }
            }
            size *= 2;
        }
        (0..=n / 2).map(|b| re[b] * re[b] + im[b] * im[b]).collect()
    }
}

// ── Results ────────────────────────────────────────────────────────────────

/// Outcome of one band search.
#[derive(Debug, Clone, Default, Serialize)]
pub struct BandEstimate {
    /// Peak frequency in Hz, `None` when no statistically separated peak.
    pub frequency_hz: Option<f64>,
    /// Combined-spectrum peak over in-band median floor.
    pub snr: f64,
    /// 0..1 confidence (noise-calibrated; see module docs).
    pub confidence: f64,
    /// Fraction of selected subcarriers whose own peak agrees with the global one.
    pub agreement: f64,
    pub subcarriers_used: usize,
    /// `"amplitude"` or `"phase"`.
    pub source: &'static str,
}

impl BandEstimate {
    pub fn rate_bpm(&self) -> Option<f64> {
        self.frequency_hz.map(|f| f * 60.0)
    }
}

/// Full spectral result for one refresh.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Estimate {
    pub breathing: BandEstimate,
    pub heartbeat: BandEstimate,
    /// Seconds of signal covered by the window when this was computed.
    pub window_secs: f64,
    /// Uniform analysis rate the window was resampled to.
    pub analysis_rate_hz: f64,
}

// ── Estimation ─────────────────────────────────────────────────────────────

fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = values.len();
    if n % 2 == 1 {
        values[n / 2]
    } else {
        0.5 * (values[n / 2 - 1] + values[n / 2])
    }
}

/// Resampled, detrended, Hann-windowed series per usable subcarrier.
struct Pool {
    series: Vec<Vec<f64>>,
}

fn build_pools(window: &SubcarrierWindow, fs: f64) -> (Pool, Option<Pool>, usize) {
    let n_frames = window.times.len();
    let t0 = window.times[0];
    let span = window.span_secs();
    let n = ((span * fs).floor() as usize + 1).max(8);
    // Bracketing index and fraction for every grid point.
    let mut idx = Vec::with_capacity(n);
    let mut frac = Vec::with_capacity(n);
    let mut cursor = 0usize;
    for j in 0..n {
        let g = t0 + j as f64 / fs;
        while cursor + 2 < n_frames && window.times[cursor + 1] < g {
            cursor += 1;
        }
        let ta = window.times[cursor];
        let tb = window.times[(cursor + 1).min(n_frames - 1)];
        let w = if tb > ta {
            ((g - ta) / (tb - ta)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        idx.push(cursor);
        frac.push(w);
    }
    let k = window.n_sub;
    let mut amp = vec![vec![0.0f64; n]; k];
    let mut pha = if window.has_phase() {
        Some(vec![vec![0.0f64; n]; k])
    } else {
        None
    };
    for j in 0..n {
        let (a, b, w) = (idx[j], (idx[j] + 1).min(n_frames - 1), frac[j] as f32);
        for s in 0..k {
            amp[s][j] = (window.amp[a][s] * (1.0 - w) + window.amp[b][s] * w) as f64;
        }
        if let Some(pha) = pha.as_mut() {
            for s in 0..k {
                pha[s][j] = (window.phase[a][s] * (1.0 - w) + window.phase[b][s] * w) as f64;
            }
        }
    }
    (
        finish_pool(amp, true),
        pha.map(|p| finish_pool(p, false)),
        n,
    )
}

/// Detrend (mean + linear), drop dead subcarriers, apply a Hann window.
fn finish_pool(raw: Vec<Vec<f64>>, relative: bool) -> Pool {
    let mut series = Vec::with_capacity(raw.len());
    for mut x in raw {
        let n = x.len();
        let nf = n as f64;
        let mean = x.iter().sum::<f64>() / nf;
        if relative && mean.abs() < 1e-9 {
            continue; // null / guard subcarrier
        }
        let mean_t = (nf - 1.0) / 2.0;
        let (mut sxy, mut sxx) = (0.0, 0.0);
        for (i, v) in x.iter().enumerate() {
            let dx = i as f64 - mean_t;
            sxy += dx * (v - mean);
            sxx += dx * dx;
        }
        let slope = if sxx > 0.0 { sxy / sxx } else { 0.0 };
        for (i, v) in x.iter_mut().enumerate() {
            *v -= mean + slope * (i as f64 - mean_t);
        }
        let var = x.iter().map(|v| v * v).sum::<f64>() / nf;
        let scale = if relative { mean.abs() } else { 1.0 };
        if !var.is_finite() || var.sqrt() / scale < 1e-9 {
            continue; // constant series carries no periodic information
        }
        for (i, v) in x.iter_mut().enumerate() {
            let w = 0.5 * (1.0 - (2.0 * PI * i as f64 / (nf - 1.0)).cos());
            *v *= w;
        }
        series.push(x);
    }
    Pool { series }
}

/// Peak search over the combined, noise-normalised spectra of one pool.
fn search_band(
    spectra: &[Vec<f64>],
    fs: f64,
    nfft: usize,
    band: (f64, f64),
    excluded: &[(f64, f64)],
    source: &'static str,
) -> BandEstimate {
    let df = fs / nfft as f64;
    let len = spectra.first().map_or(0, Vec::len);
    if len < 4 {
        return BandEstimate::default();
    }
    let min_bin = (band.0 / df).ceil() as usize;
    let max_bin = ((band.1 / df).floor() as usize).min(len - 2);
    if max_bin <= min_bin + 8 {
        return BandEstimate::default();
    }
    let bins: Vec<usize> = (min_bin..=max_bin)
        .filter(|b| {
            let f = *b as f64 * df;
            !excluded.iter().any(|(lo, hi)| f >= *lo && f <= *hi)
        })
        .collect();
    if bins.len() < 8 {
        return BandEstimate::default();
    }

    // Per-subcarrier screening: peak-to-median ratio inside the band.
    struct Candidate {
        score: f64,
        spectrum: usize,
        median: f64,
        peak_bin: usize,
    }
    let mut candidates: Vec<Candidate> = Vec::with_capacity(spectra.len());
    let mut scratch = Vec::with_capacity(bins.len());
    for (i, p) in spectra.iter().enumerate() {
        scratch.clear();
        scratch.extend(bins.iter().map(|&b| p[b]));
        let med = median(&mut scratch);
        if !(med > 1e-18) {
            continue;
        }
        let (peak_bin, peak) = bins
            .iter()
            .map(|&b| (b, p[b]))
            .fold((bins[0], 0.0), |acc, cur| if cur.1 > acc.1 { cur } else { acc });
        candidates.push(Candidate {
            score: peak / med,
            spectrum: i,
            median: med,
            peak_bin,
        });
    }
    if candidates.is_empty() {
        return BandEstimate::default();
    }
    candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    let k = (candidates.len() / 4)
        .clamp(3, SELECTED_SUBCARRIERS_MAX)
        .min(candidates.len());
    let selected = &candidates[..k];

    let mut combined = vec![0.0f64; len];
    for c in selected {
        let p = &spectra[c.spectrum];
        for &b in &bins {
            combined[b] += p[b] / c.median;
        }
    }
    scratch.clear();
    scratch.extend(bins.iter().map(|&b| combined[b]));
    let floor = median(&mut scratch);
    if !(floor > 1e-18) {
        return BandEstimate::default();
    }
    let (peak_bin, peak) = bins
        .iter()
        .map(|&b| (b, combined[b]))
        .fold((bins[0], 0.0), |acc, cur| if cur.1 > acc.1 { cur } else { acc });
    let ratio = peak / floor;

    // How many selected subcarriers independently peak at the same frequency.
    let tol_bins = ((0.04 / df).ceil() as usize).max(2);
    let agree = selected
        .iter()
        .filter(|c| c.peak_bin.abs_diff(peak_bin) <= tol_bins)
        .count();
    let agreement = agree as f64 / k as f64;

    // Parabolic interpolation on the magnitude of the combined spectrum.
    let interior = peak_bin > min_bin
        && peak_bin < max_bin
        && bins.contains(&(peak_bin - 1))
        && bins.contains(&(peak_bin + 1));
    let freq = if interior {
        let (a, b, c) = (
            combined[peak_bin - 1].sqrt(),
            combined[peak_bin].sqrt(),
            combined[peak_bin + 1].sqrt(),
        );
        let denom = a - 2.0 * b + c;
        let shift = if denom.abs() > f64::EPSILON {
            (0.5 * (a - c) / denom).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        (peak_bin as f64 + shift) * df
    } else {
        peak_bin as f64 * df
    };

    // Noise-calibrated confidence. With K averaged exponential periodograms the
    // expected max/median under noise is ~1 + 2.5/sqrt(K); the margin above
    // that, relative to that level, maps to 0..1.
    let r_noise = 1.0 + 2.5 / (k as f64).sqrt();
    let conf_ratio = ((ratio - r_noise) / (2.0 * r_noise)).clamp(0.0, 1.0);
    let edge = peak_bin <= min_bin + 1 || peak_bin + 1 >= max_bin;
    let mut confidence = conf_ratio * (0.4 + 0.6 * agreement);
    if agree < 2 && k >= 3 {
        confidence = confidence.min(0.2);
    }
    if edge {
        confidence *= 0.4; // edge peaks are usually leakage from outside the band
    }
    confidence = confidence.clamp(0.0, 1.0);

    BandEstimate {
        frequency_hz: (confidence >= MIN_REPORTED_CONFIDENCE).then_some(freq),
        snr: ratio,
        confidence,
        agreement,
        subcarriers_used: k,
        source,
    }
}

fn better(a: BandEstimate, b: BandEstimate) -> BandEstimate {
    if (b.confidence, b.snr) > (a.confidence, a.snr) {
        b
    } else {
        a
    }
}

fn spectra_for(pool: &Pool, plan: &FftPlan) -> Vec<Vec<f64>> {
    let mut re = Vec::new();
    let mut im = Vec::new();
    pool.series
        .iter()
        .map(|x| plan.power(x, &mut re, &mut im))
        .collect()
}

/// Run the full breathing + heartbeat estimation over `window`.
pub fn estimate(window: &SubcarrierWindow, plan_slot: &mut Option<FftPlan>) -> Estimate {
    let n_frames = window.len();
    let span = window.span_secs();
    if n_frames < 8 || span < MIN_BREATHING_SECS.min(MIN_HEARTBEAT_SECS) {
        return Estimate {
            window_secs: span,
            ..Estimate::default()
        };
    }
    let fs = ((n_frames as f64 - 1.0) / span).clamp(1.0, MAX_ANALYSIS_RATE_HZ);
    let (amp_pool, phase_pool, n) = build_pools(window, fs);
    let nfft = (n.next_power_of_two() * 2).max(256);
    if plan_slot.as_ref().map(FftPlan::len) != Some(nfft) {
        *plan_slot = Some(FftPlan::new(nfft));
    }
    let plan = plan_slot.as_ref().expect("plan just installed");

    let amp_spectra = spectra_for(&amp_pool, plan);
    let phase_spectra = phase_pool.as_ref().map(|p| spectra_for(p, plan));

    let mut breathing = BandEstimate::default();
    if span >= MIN_BREATHING_SECS && fs >= MIN_BREATH_ANALYSIS_RATE_HZ {
        breathing = search_band(&amp_spectra, fs, nfft, BREATHING_BAND_HZ, &[], "amplitude");
        if let Some(ps) = &phase_spectra {
            breathing = better(
                breathing,
                search_band(ps, fs, nfft, BREATHING_BAND_HZ, &[], "phase"),
            );
        }
    }

    let mut heartbeat = BandEstimate::default();
    if span >= MIN_HEARTBEAT_SECS && fs >= MIN_HEART_ANALYSIS_RATE_HZ {
        let df = fs / nfft as f64;
        let mut notches = Vec::new();
        if let (Some(fb), true) = (breathing.frequency_hz, breathing.confidence >= 0.3) {
            let tol = HARMONIC_TOLERANCE_HZ.max(1.5 * df);
            for h in 2..=5 {
                let c = fb * h as f64;
                notches.push((c - tol, c + tol));
            }
        }
        heartbeat = search_band(&amp_spectra, fs, nfft, HEARTBEAT_BAND_HZ, &notches, "amplitude");
        if let Some(ps) = &phase_spectra {
            heartbeat = better(
                heartbeat,
                search_band(ps, fs, nfft, HEARTBEAT_BAND_HZ, &notches, "phase"),
            );
        }
        heartbeat.confidence = heartbeat.confidence.min(HEART_CONFIDENCE_CAP);
        if heartbeat.confidence < MIN_REPORTED_CONFIDENCE {
            heartbeat.frequency_hz = None;
        }
    }

    Estimate {
        breathing,
        heartbeat,
        window_secs: span,
        analysis_rate_hz: fs,
    }
}

// ── Tests (SYNTHETIC) ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vital_signs::VitalSignDetector;

    /// Small deterministic generator so the tests need no RNG dependency.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
        }
        fn gauss(&mut self) -> f64 {
            let (u1, u2) = (self.next().max(1e-12), self.next());
            (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos()
        }
    }

    /// Synthetic CSI: breathing / heartbeat with per-subcarrier gain, sign and
    /// phase, plus noise, a per-frame linear phase slope and optional jitter.
    fn run(
        breath_hz: Option<f64>,
        heart_hz: Option<f64>,
        secs: f64,
        fs: f64,
        jitter: f64,
        noise: f64,
        seed: u64,
    ) -> VitalSignDetector {
        let mut rng = Lcg(seed);
        let mut det = VitalSignDetector::new(fs);
        det.estimate_interval_secs = 1e9;
        let n_sub = 56;
        let gains: Vec<(f64, f64)> = (0..n_sub)
            .map(|_| ((rng.next() * 2.0 - 1.0), rng.next() * 2.0 * PI))
            .collect();
        let n = (secs * fs) as usize;
        for f in 0..n {
            let t = f as f64 / fs + jitter * (rng.next() - 0.5) * 2.0;
            let slope = (rng.next() - 0.5) * 0.6; // per-frame STO-like slope
            let offset = rng.next() * 2.0 * PI; // per-frame CFO-like offset
            let mut amp = Vec::with_capacity(n_sub);
            let mut pha = Vec::with_capacity(n_sub);
            for (s, (g, ph0)) in gains.iter().enumerate() {
                let b = breath_hz
                    .map(|h| 0.8 * g * (2.0 * PI * h * t + ph0).sin())
                    .unwrap_or(0.0);
                let hb = heart_hz
                    .map(|h| 0.12 * g * (2.0 * PI * h * t + 1.3 * ph0).sin())
                    .unwrap_or(0.0);
                amp.push(20.0 + 3.0 * (s as f64 * 0.17).sin() + b + hb + noise * rng.gauss());
                let phase_signal = breath_hz
                    .map(|h| 0.05 * g * (2.0 * PI * h * t + ph0).sin())
                    .unwrap_or(0.0)
                    + heart_hz
                        .map(|h| 0.015 * g * (2.0 * PI * h * t + ph0).sin())
                        .unwrap_or(0.0);
                let raw = offset + slope * s as f64 + phase_signal + 0.01 * noise * rng.gauss();
                pha.push((raw + PI).rem_euclid(2.0 * PI) - PI);
            }
            det.process_frame_at(&amp, &pha, t);
        }
        det.force_estimate();
        det
    }

    #[test]
    fn finds_breathing_and_heart_with_random_sign_and_phase() {
        let det = run(Some(0.27), Some(1.25), 30.0, 20.0, 0.0, 0.25, 7);
        let est = det.last_estimate();
        let br = est.breathing.rate_bpm().expect("breathing detected");
        let hr = est.heartbeat.rate_bpm().expect("heart detected");
        assert!((br - 16.2).abs() < 1.0, "breathing {br}");
        assert!((hr - 75.0).abs() < 3.0, "heart {hr}");
        assert!(est.breathing.confidence > 0.55, "{:?}", est.breathing);
        assert!(est.heartbeat.confidence > 0.55, "{:?}", est.heartbeat);
    }

    #[test]
    fn tolerates_arrival_jitter() {
        let det = run(Some(0.3), Some(1.1), 30.0, 25.0, 0.008, 0.25, 11);
        let est = det.last_estimate();
        assert!((est.breathing.rate_bpm().unwrap() - 18.0).abs() < 1.2);
        assert!((est.heartbeat.rate_bpm().unwrap() - 66.0).abs() < 3.5);
    }

    #[test]
    fn breathing_harmonic_is_not_reported_as_heart_rate() {
        // 0.25 Hz breathing: its 4th harmonic lies at 1.0 Hz inside the heart band.
        let mut rng = Lcg(3);
        let mut det = VitalSignDetector::new(20.0);
        det.estimate_interval_secs = 1e9;
        for f in 0..600 {
            let t = f as f64 / 20.0;
            let amp: Vec<f64> = (0..56)
                .map(|s| {
                    let fund = (2.0 * PI * 0.25 * t).sin();
                    // Strongly non-sinusoidal chest motion => harmonics.
                    let shaped = fund + 0.6 * (2.0 * PI * 0.5 * t).sin()
                        + 0.4 * (2.0 * PI * 0.75 * t).sin()
                        + 0.35 * (2.0 * PI * 1.0 * t).sin();
                    20.0 + (s as f64 * 0.1).sin() + shaped + 0.2 * rng.gauss()
                })
                .collect();
            det.process_frame_at(&amp, &[], t);
        }
        det.force_estimate();
        let est = det.last_estimate();
        assert!(est.breathing.rate_bpm().is_some());
        assert!(
            est.heartbeat.rate_bpm().is_none_or(|bpm| (bpm - 60.0).abs() > 2.5),
            "harmonic leaked as heart rate: {:?}",
            est.heartbeat
        );
    }

    #[test]
    fn noise_only_rarely_clears_the_publication_gate() {
        let trials = 24;
        let mut false_breath = 0;
        let mut false_heart = 0;
        for seed in 0..trials {
            let det = run(None, None, 30.0, 20.0, 0.004, 1.0, 1000 + seed);
            let est = det.last_estimate();
            if est.breathing.confidence >= 0.55 {
                false_breath += 1;
            }
            if est.heartbeat.confidence >= 0.55 {
                false_heart += 1;
            }
        }
        assert!(false_breath <= 1, "breathing false alarms: {false_breath}/{trials}");
        assert!(false_heart <= 1, "heart false alarms: {false_heart}/{trials}");
    }

    #[test]
    fn a_long_gap_discards_stale_history() {
        let mut w = SubcarrierWindow::default();
        let amp = vec![10.0; 8];
        for i in 0..40 {
            assert!(w.push(i as f64 * 0.05, &amp, &[]));
        }
        assert!(w.len() == 40);
        assert!(w.push(10.0, &amp, &[]));
        assert_eq!(w.len(), 1, "samples across a >2 s gap must not be mixed");
        assert!(!w.push(9.0, &amp, &[]), "time must be monotonic");
    }

    #[test]
    fn phase_sanitising_removes_slope_and_offset() {
        let phase: Vec<f64> = (0..32)
            .map(|i| ((1.1 + 0.4 * i as f64) + PI).rem_euclid(2.0 * PI) - PI)
            .collect();
        let mut out = Vec::new();
        sanitize_phase(&phase, &mut out);
        assert_eq!(out.len(), 32);
        assert!(out.iter().all(|v| v.abs() < 1e-4), "{out:?}");
    }

    #[test]
    fn fft_plan_matches_a_pure_tone() {
        let n = 256;
        let plan = FftPlan::new(n);
        let x: Vec<f64> = (0..n).map(|i| (2.0 * PI * 8.0 * i as f64 / n as f64).sin()).collect();
        let (mut re, mut im) = (Vec::new(), Vec::new());
        let p = plan.power(&x, &mut re, &mut im);
        let peak = p
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;
        assert_eq!(peak, 8);
    }
}
