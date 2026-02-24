// daw_modules/src/effects/equalizer.rs

use biquad::*;
use serde::{Deserialize, Serialize};

// 1. Supported Filter Types
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EqFilterType {
    LowPass,
    HighPass,
    BandPass,
    Notch,
    Peaking,
    LowShelf,
    HighShelf,
}

// 2. Parameters
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EqParams {
    pub filter_type: EqFilterType,
    pub freq: f32, // Hz
    pub q: f32,    // Q-Factor
    pub gain: f32, // dB
    pub active: bool,
}

impl Default for EqParams {
    fn default() -> Self {
        Self {
            filter_type: EqFilterType::Peaking,
            freq: 1000.0,
            q: 0.707,
            gain: 0.0,
            active: false,
        }
    }
}

// 3. The DSP Processor
pub struct EqBand {
    coeffs: Coefficients<f32>,
    filters: Vec<DirectForm2Transposed<f32>>,
    pub params: EqParams,
    sr: u32,
}

impl EqBand {
    pub fn new(sr: u32, channels: usize, params: EqParams) -> Self {
        // Init with safe default
        let coeffs = Coefficients::<f32>::from_params(
            Type::PeakingEQ(0.0.into()), 
            sr.hz(),
            1000.0.hz(),
            0.707.into()
        ).unwrap();

        let mut filters = Vec::with_capacity(channels);
        for _ in 0..channels {
            filters.push(DirectForm2Transposed::<f32>::new(coeffs));
        }
        
        let mut band = Self {
            coeffs,
            filters,
            params,
            sr,
        };
        band.update_coefficients(true); 
        band
    }

    pub fn set_sr(&mut self, sr: u32) {
        self.sr = sr;
        self.update_coefficients(true);
    }

    pub fn update(&mut self, new_params: EqParams) {
        let type_changed = self.params.filter_type != new_params.filter_type;
        self.params = new_params;
        self.update_coefficients(type_changed);
    }

    fn update_coefficients(&mut self, reset_state: bool) {
        // --- SAFETY CLAMPS ---
        // Freq must be < SampleRate / 2 (Nyquist)
        let safe_freq = self.params.freq.clamp(20.0, (self.sr as f32 / 2.0) - 1.0);
        // Q must be > 0
        let safe_q = self.params.q.max(0.1); 
        
        // Debug Log important changes
        // println!("EQ UPDATE: Type {:?} | Freq {:.1} | Q {:.2} | Gain {:.1}dB", 
        //    self.params.filter_type, safe_freq, safe_q, self.params.gain);

        let biquad_type = match self.params.filter_type {
            EqFilterType::LowPass => Type::LowPass,
            EqFilterType::HighPass => Type::HighPass,
            EqFilterType::BandPass => Type::BandPass,
            EqFilterType::Notch => Type::Notch,
            EqFilterType::Peaking => Type::PeakingEQ(self.params.gain.into()),
            EqFilterType::LowShelf => Type::LowShelf(self.params.gain.into()),
            EqFilterType::HighShelf => Type::HighShelf(self.params.gain.into()),
        };

        if let Ok(new_coeffs) = Coefficients::<f32>::from_params(
            biquad_type,
            self.sr.hz(),
            safe_freq.hz(),
            safe_q.into()
        ) {
            self.coeffs = new_coeffs;
            
            for filter in &mut self.filters {
                if reset_state {
                    filter.reset_state();
                }
                filter.update_coefficients(self.coeffs);
            }
        } else {
            println!("⚠️ Failed to calculate coefficients for Freq {}", safe_freq);
        }
    }

    #[inline]
    pub fn process(&mut self, sample: f32, channel_idx: usize) -> f32 {
        if self.params.active {
            if let Some(filter) = self.filters.get_mut(channel_idx) {
                let out = filter.run(sample);
                // Denormal protection
                if out.abs() < 1e-20 { return 0.0; }
                return out;
            }
        }
        sample
    }
}

// 4. The Chain
pub struct TrackEq {
    bands: Vec<EqBand>,
}

impl TrackEq {
    pub fn new(sr: u32, channels: usize) -> Self {
        let mut bands = Vec::with_capacity(4);

        // Band 1: HPF 
        bands.push(EqBand::new(sr, channels, EqParams {
            filter_type: EqFilterType::HighPass,
            freq: 75.0,
            q: 0.707,
            gain: 0.0,
            active: true, 
        }));

        // Band 2: Peaking
        bands.push(EqBand::new(sr, channels, EqParams {
            filter_type: EqFilterType::Peaking,
            freq: 200.0,
            q: 1.0,
            gain: 0.0,
            active: false,
        }));

        // Band 3: Peaking
        bands.push(EqBand::new(sr, channels, EqParams {
            filter_type: EqFilterType::Peaking,
            freq: 2000.0,
            q: 1.0,
            gain: 0.0,
            active: false,
        }));

        // Band 4: High Shelf
        bands.push(EqBand::new(sr, channels, EqParams {
            filter_type: EqFilterType::HighShelf,
            freq: 10000.0,
            q: 0.707,
            gain: 0.0,
            active: false,
        }));

        Self { bands }
    }

    pub fn update_band(&mut self, index: usize, params: EqParams) {
        if let Some(band) = self.bands.get_mut(index) {
            band.update(params);
        }
    }

    // Zero-allocation in-place processing
    pub fn process_buffer(&mut self, buffer: &mut [f32], channels: usize) {
        for frame in buffer.chunks_mut(channels) {
            for (ch, sample) in frame.iter_mut().enumerate() {
                let mut s = *sample;
                for band in &mut self.bands {
                    s = band.process(s, ch);
                }
                *sample = s;
            }
        }
    }

    pub fn get_state(&self) -> Vec<EqParams> {
        self.bands.iter().map(|b| b.params).collect()
    }

    pub fn set_state(&mut self, state: Vec<EqParams>) {
        // Loop through the saved parameters and apply them to the corresponding bands
        for (i, params) in state.into_iter().enumerate() {
            // Check to make sure we don't exceed the number of bands your EQ supports
            if i < self.bands.len() {
                self.bands[i].update(params); // <--- CHANGED FROM set_params TO update
            }
        }
    }
}