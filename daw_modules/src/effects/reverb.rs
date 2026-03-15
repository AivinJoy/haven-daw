// daw_modules/src/effects/reverb.rs

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use serde::{Deserialize, Serialize};

// Standard Freeverb tuning constants (adjusted for 44.1kHz baseline, scaled by sample rate later)
const COMB_TUNING: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNING: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;
const MAX_PRE_DELAY_MS: f32 = 500.0;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ReverbParams {
    pub is_active: bool,
    pub room_size: f32,    // 0.0 to 1.0
    pub damping: f32,      // 0.0 to 1.0
    pub mix: f32,          // 0.0 (Dry) to 1.0 (Wet)
    pub width: f32,        // 0.0 (Mono) to 1.0 (Stereo)
    pub pre_delay_ms: f32, // 0.0 to 500.0
    pub low_cut_hz: f32,   // 20.0 to 1000.0
    pub high_cut_hz: f32,  // 1000.0 to 20000.0
}

fn f32_to_atomic(val: f32) -> AtomicU32 {
    AtomicU32::new(val.to_bits())
}

fn atomic_to_f32(atomic: &AtomicU32) -> f32 {
    f32::from_bits(atomic.load(Ordering::Relaxed))
}

// --- Allocation-Free Delay Lines ---

struct DelayLine {
    buffer: Vec<f32>,
    index: usize,
}

impl DelayLine {
    fn new(size: usize) -> Self {
        Self {
            buffer: vec![0.0; size],
            index: 0,
        }
    }

    #[inline]
    fn read(&self) -> f32 {
        self.buffer[self.index]
    }

    #[inline]
    fn write_and_advance(&mut self, value: f32) {
        self.buffer[self.index] = value;
        self.index = (self.index + 1) % self.buffer.len();
    }
}

struct CombFilter {
    delay: DelayLine,
    filter_store: f32,
}

impl CombFilter {
    fn new(size: usize) -> Self {
        Self { delay: DelayLine::new(size), filter_store: 0.0 }
    }

    #[inline]
    fn process(&mut self, input: f32, damp: f32, feedback: f32) -> f32 {
        let output = self.delay.read();
        self.filter_store = (output * (1.0 - damp)) + (self.filter_store * damp);
        self.delay.write_and_advance(input + (self.filter_store * feedback));
        output
    }
}

struct AllpassFilter {
    delay: DelayLine,
}

impl AllpassFilter {
    fn new(size: usize) -> Self {
        Self { delay: DelayLine::new(size) }
    }

    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        let delayed = self.delay.read();
        let output = -input + delayed;
        // Standard allpass feedback coefficient is approx 0.5
        self.delay.write_and_advance(input + (delayed * 0.5));
        output
    }
}

// --- One-Pole Filters for EQ ---

struct OnePoleFilter {
    z1: f32,
}

impl OnePoleFilter {
    fn new() -> Self { Self { z1: 0.0 } }

    #[inline]
    fn process_lp(&mut self, input: f32, a0: f32, b1: f32) -> f32 {
        self.z1 = input * a0 + self.z1 * b1;
        self.z1
    }

    #[inline]
    fn process_hp(&mut self, input: f32, a0: f32, b1: f32) -> f32 {
        let lp = self.process_lp(input, a0, b1);
        input - lp
    }
}

// --- The Main Processor ---

pub struct ReverbNode {
    // Lock-free Atomics
    is_active: AtomicBool,
    room_size: AtomicU32,
    damping: AtomicU32,
    mix: AtomicU32,
    width: AtomicU32,
    pre_delay_ms: AtomicU32,
    low_cut_hz: AtomicU32,
    high_cut_hz: AtomicU32,

    sample_rate: f32,

    // Pre-allocated DSP buffers
    pre_delay_buffer: DelayLine,
    
    combs_l: Vec<CombFilter>,
    combs_r: Vec<CombFilter>,
    allpasses_l: Vec<AllpassFilter>,
    allpasses_r: Vec<AllpassFilter>,

    // EQ Filters
    hp_l: OnePoleFilter,
    lp_l: OnePoleFilter,
}

impl ReverbNode {
    pub fn new(sample_rate: f32) -> Self {
        let sr_scale = sample_rate / 44100.0;
        
        let mut combs_l = Vec::with_capacity(8);
        let mut combs_r = Vec::with_capacity(8);
        for &tune in COMB_TUNING.iter() {
            let l_len = (tune as f32 * sr_scale) as usize;
            let r_len = ((tune + STEREO_SPREAD) as f32 * sr_scale) as usize;
            combs_l.push(CombFilter::new(l_len));
            combs_r.push(CombFilter::new(r_len));
        }

        let mut allpasses_l = Vec::with_capacity(4);
        let mut allpasses_r = Vec::with_capacity(4);
        for &tune in ALLPASS_TUNING.iter() {
            let l_len = (tune as f32 * sr_scale) as usize;
            let r_len = ((tune + STEREO_SPREAD) as f32 * sr_scale) as usize;
            allpasses_l.push(AllpassFilter::new(l_len));
            allpasses_r.push(AllpassFilter::new(r_len));
        }

        let max_pre_delay_samples = (MAX_PRE_DELAY_MS * sample_rate / 1000.0) as usize;

        Self {
            is_active: AtomicBool::new(false),
            room_size: f32_to_atomic(0.8),
            damping: f32_to_atomic(0.5),
            mix: f32_to_atomic(0.3),
            width: f32_to_atomic(1.0),
            pre_delay_ms: f32_to_atomic(10.0),
            low_cut_hz: f32_to_atomic(100.0),
            high_cut_hz: f32_to_atomic(8000.0),
            
            sample_rate,
            pre_delay_buffer: DelayLine::new(max_pre_delay_samples),
            
            combs_l, combs_r,
            allpasses_l, allpasses_r,

            hp_l: OnePoleFilter::new(),
            lp_l: OnePoleFilter::new(),
        }
    }

    // --- Dynamic Param Setters (For your new generic routing system) ---
    pub fn set_param(&self, param_name: &str, value: f32) {
        match param_name {
            "room_size" => self.room_size.store(value.to_bits(), Ordering::Relaxed),
            "damping" => self.damping.store(value.to_bits(), Ordering::Relaxed),
            "mix" => self.mix.store(value.to_bits(), Ordering::Relaxed),
            "width" => self.width.store(value.to_bits(), Ordering::Relaxed),
            "pre_delay" => self.pre_delay_ms.store(value.to_bits(), Ordering::Relaxed),
            "low_cut" => self.low_cut_hz.store(value.to_bits(), Ordering::Relaxed),
            "high_cut" => self.high_cut_hz.store(value.to_bits(), Ordering::Relaxed),
            "active" => self.is_active.store(value > 0.5, Ordering::Relaxed),
            _ => {}
        }
    }

    pub fn get_params(&self) -> ReverbParams {
        ReverbParams {
            is_active: self.is_active.load(Ordering::Relaxed),
            room_size: atomic_to_f32(&self.room_size),
            damping: atomic_to_f32(&self.damping),
            mix: atomic_to_f32(&self.mix),
            width: atomic_to_f32(&self.width),
            pre_delay_ms: atomic_to_f32(&self.pre_delay_ms),
            low_cut_hz: atomic_to_f32(&self.low_cut_hz),
            high_cut_hz: atomic_to_f32(&self.high_cut_hz),
        }
    }

    // --- ZERO ALLOCATION AUDIO LOOP ---
    #[inline]
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        if !self.is_active.load(Ordering::Relaxed) {
            return (left, right);
        }

        // 1. Read atomics (Lock-free)
        let room_size = atomic_to_f32(&self.room_size) * 0.28 + 0.7; // Scale to Freeverb range
        let damp = atomic_to_f32(&self.damping) * 0.4;
        let mix = atomic_to_f32(&self.mix);
        let width = atomic_to_f32(&self.width);
        let wet1 = mix * (width / 2.0 + 0.5);
        let wet2 = mix * ((1.0 - width) / 2.0);
        let dry = 1.0 - mix;

        // EQ Coefficients calculation (Simple one-pole mapping)
        let hc_rad = (2.0 * std::f32::consts::PI * atomic_to_f32(&self.high_cut_hz)) / self.sample_rate;
        let b1_lp = (-hc_rad).exp();
        let a0_lp = 1.0 - b1_lp;

        let lc_rad = (2.0 * std::f32::consts::PI * atomic_to_f32(&self.low_cut_hz)) / self.sample_rate;
        let b1_hp = (-lc_rad).exp();
        let a0_hp = 1.0 - b1_hp;

        // 2. Pre-Delay
        let input_mono = (left + right) * 0.5;
        let pre_delayed = self.pre_delay_buffer.read();
        
        // Write to pre-delay. Index wrapping based on actual pre-delay parameter.
        let pd_samples = (atomic_to_f32(&self.pre_delay_ms) * self.sample_rate / 1000.0) as usize;
        let pd_write_idx = (self.pre_delay_buffer.index + pd_samples).clamp(0, self.pre_delay_buffer.buffer.len() - 1);
        self.pre_delay_buffer.buffer[pd_write_idx] = input_mono;
        self.pre_delay_buffer.index = (self.pre_delay_buffer.index + 1) % self.pre_delay_buffer.buffer.len();

        // 3. Apply Reverb Input EQ (Low Cut / High Cut)
        let mut reverb_input = pre_delayed;
        reverb_input = self.hp_l.process_hp(reverb_input, a0_hp, b1_hp);
        reverb_input = self.lp_l.process_lp(reverb_input, a0_lp, b1_lp);

        // Attenuate input to avoid clipping in the combs
        let reverb_input = reverb_input * 0.015;

        // 4. Parallel Comb Filters
        let mut out_l = 0.0;
        let mut out_r = 0.0;
        
        for i in 0..8 {
            out_l += self.combs_l[i].process(reverb_input, damp, room_size);
            out_r += self.combs_r[i].process(reverb_input, damp, room_size);
        }

        // 5. Series Allpass Filters
        for i in 0..4 {
            out_l = self.allpasses_l[i].process(out_l);
            out_r = self.allpasses_r[i].process(out_r);
        }

        // 6. Stereo Mix Matrix
        let final_l = (out_l * wet1) + (out_r * wet2) + (left * dry);
        let final_r = (out_r * wet1) + (out_l * wet2) + (right * dry);

        (final_l, final_r)
    }
}