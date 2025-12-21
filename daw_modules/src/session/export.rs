// daw_modules/src/session/export.rs

use crate::session::serialization::ProjectManifest;
use crate::decoder::{pipe, resample};
use anyhow::Result;
use hound::{WavSpec, WavWriter, SampleFormat};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::formats::FormatReader;
use symphonia::core::codecs::Decoder;
use rubato::Resampler; 

pub struct ExportVoice {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    resampler: Option<rubato::SincFixedIn<f32>>,
    
    input_sample_buf: Option<SampleBuffer<f32>>,
    resampler_input_buffer: Vec<f32>, 
    output_buffer: Vec<f32>,          
    
    finished: bool,
    source_channels: usize,
    
    start_frame: usize,
    frames_processed: usize,
    
    gain: f32,
    pan: f32,
    muted: bool,
}

impl ExportVoice {
    // FIX: Added start_time to arguments
    pub fn new(path: &str, target_sample_rate: u32, start_time: f64) -> Result<Self> {
        // FIX: Reverted to standard open_and_probe (returns 2 items, not 3)
        let (format, track_id) = pipe::open_and_probe(path)?;
        
        let track = format.tracks().iter().find(|t| t.id == track_id).unwrap();
        let source_rate = track.codec_params.sample_rate.unwrap_or(44100);
        let source_channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2);

        let mut format = format; 
        let decoder = pipe::make_decoder(&mut *format)?;
        
        let resampler = if source_rate != target_sample_rate {
            resample::build_resampler(source_rate, target_sample_rate, 2)?
        } else {
            None
        };

        let start_frame = (start_time * target_sample_rate as f64).round() as usize;

        Ok(Self {
            format,
            decoder,
            track_id,
            resampler,
            input_sample_buf: None,
            resampler_input_buffer: Vec::with_capacity(4096),
            output_buffer: Vec::new(),
            finished: false,
            source_channels,
            gain: 1.0,
            pan: 0.0,
            muted: false,
            start_frame,
            frames_processed: 0, // FIX: Comma instead of semicolon
        })
    }

    fn prepare_samples(&mut self, frames_needed: usize) -> Result<bool> {
        while self.output_buffer.len() < frames_needed * 2 {
            if self.finished && self.resampler_input_buffer.is_empty() {
                break;
            }

            let chunk_size = if let Some(r) = &self.resampler { r.input_frames_next() } else { 0 };
            let needed_for_resample = if chunk_size > 0 { chunk_size * 2 } else { 0 };

            if !self.finished && (self.resampler.is_none() || self.resampler_input_buffer.len() < needed_for_resample) {
                let packet_opt = loop {
                    match self.format.next_packet() {
                        Ok(p) => {
                            if p.track_id() == self.track_id { break Some(p); }
                        },
                        Err(_) => {
                            self.finished = true;
                            break None; 
                        }
                    }
                };

                if let Some(packet) = packet_opt {
                    let decoded = match self.decoder.decode(&packet) {
                        Ok(d) => d,
                        Err(_) => continue,
                    };

                    if self.input_sample_buf.is_none() {
                        let spec = *decoded.spec();
                        self.input_sample_buf = Some(SampleBuffer::new(decoded.capacity() as u64 + 4096, spec));
                    }
                    let buf = self.input_sample_buf.as_mut().unwrap();
                    buf.copy_interleaved_ref(decoded);
                    let samples = buf.samples();

                    if self.source_channels == 1 {
                        for s in samples {
                            self.resampler_input_buffer.push(*s);
                            self.resampler_input_buffer.push(*s);
                        }
                    } else {
                        self.resampler_input_buffer.extend_from_slice(samples);
                    }
                }
            }

            if let Some(r) = &mut self.resampler {
                let chunk_frames = r.input_frames_next();
                let chunk_samples = chunk_frames * 2;

                while self.resampler_input_buffer.len() >= chunk_samples {
                    let chunk_slice = &self.resampler_input_buffer[0..chunk_samples];
                    let mut planar = vec![Vec::with_capacity(chunk_frames); 2];
                    for i in 0..chunk_frames {
                        planar[0].push(chunk_slice[i*2]);
                        planar[1].push(chunk_slice[i*2+1]);
                    }

                    if let Ok(resampled) = r.process(&planar, None) {
                         let out_frames = resampled[0].len();
                         for i in 0..out_frames {
                             self.output_buffer.push(resampled[0][i]);
                             self.output_buffer.push(resampled[1][i]);
                         }
                    }
                    self.resampler_input_buffer.drain(0..chunk_samples);
                }

                if self.finished && !self.resampler_input_buffer.is_empty() {
                    let remaining_samples = self.resampler_input_buffer.len();
                    let remaining_frames = remaining_samples / 2;
                    
                    if remaining_frames > 0 {
                        let mut planar = vec![Vec::with_capacity(remaining_frames); 2];
                        for i in 0..remaining_frames {
                            planar[0].push(self.resampler_input_buffer[i*2]);
                            planar[1].push(self.resampler_input_buffer[i*2+1]);
                        }
                        
                        if let Ok(resampled) = r.process_partial::<Vec<f32>>(Some(&planar), None) {
                             if !resampled.is_empty() {
                                 let out_frames = resampled[0].len();
                                 for i in 0..out_frames {
                                     self.output_buffer.push(resampled[0][i]);
                                     self.output_buffer.push(resampled[1][i]);
                                 }
                             }
                        }
                    }
                    self.resampler_input_buffer.clear();
                }
            } else {
                self.output_buffer.append(&mut self.resampler_input_buffer);
                if self.finished && self.output_buffer.is_empty() { break; }
            }

            if self.output_buffer.len() >= frames_needed * 2 { break; }
            if self.finished && self.resampler_input_buffer.is_empty() { break; }
        }
        Ok(!self.output_buffer.is_empty())
    }

    pub fn add_to_mix(&mut self, out_buf: &mut [f32], frames: usize) -> Result<()> {
        if self.muted { 
            self.frames_processed += frames;
            return Ok(()); 
        }

        let mut buf_offset = 0;
        if self.frames_processed < self.start_frame {
            let silence_needed = self.start_frame - self.frames_processed;
            if silence_needed >= frames {
                self.frames_processed += frames;
                return Ok(());
            } else {
                buf_offset = silence_needed;
                self.frames_processed += silence_needed;
            }
        }

        let audio_frames_needed = frames - buf_offset;
        self.prepare_samples(audio_frames_needed)?;

        let samples_available = self.output_buffer.len() / 2; 
        let frames_to_mix = audio_frames_needed.min(samples_available);

        let pan = self.pan.clamp(-1.0, 1.0);
        let (pan_l, pan_r) = if self.pan != 0.0 {
            let angle = (pan + 1.0) * 0.25 * std::f32::consts::PI;
            (angle.cos(), angle.sin())
        } else {
            (1.0, 1.0)
        };

        for i in 0..frames_to_mix {
            let out_idx = (buf_offset + i) * 2;
            let in_idx = i * 2;
            let l = self.output_buffer[in_idx] * self.gain * pan_l;
            let r = self.output_buffer[in_idx+1] * self.gain * pan_r;
            out_buf[out_idx] += l;
            out_buf[out_idx+1] += r;
        }

        if frames_to_mix > 0 {
             self.output_buffer.drain(0..(frames_to_mix * 2));
        }
        self.frames_processed += audio_frames_needed;
        Ok(())
    }
    
    pub fn is_finished(&self) -> bool {
        let started = self.frames_processed >= self.start_frame;
        started && self.finished && self.output_buffer.is_empty() && self.resampler_input_buffer.is_empty()
    }
}

pub fn export_project_to_wav(manifest: &ProjectManifest, output_path: &str) -> Result<()> {
    println!("🚀 Starting Export: {}", output_path);
    let sample_rate = 44100;
    let spec = WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(output_path, spec)?;

    let mut voices = Vec::new();
    for t_state in &manifest.tracks {
        // FIX: Correctly passing start_time from manifest
        if let Ok(mut v) = ExportVoice::new(&t_state.path, sample_rate, t_state.start_time) {
            v.gain = t_state.gain;
            v.pan = t_state.pan;
            v.muted = t_state.muted; 
            voices.push(v);
        } else {
             eprintln!("⚠️ Failed to load {}", t_state.path);
        }
    }

    // Solo Logic
    let any_solo = manifest.tracks.iter().any(|t| t.solo);
    if any_solo {
        for (i, v) in voices.iter_mut().enumerate() {
            if i < manifest.tracks.len() {
                if !manifest.tracks[i].solo { v.muted = true; } 
                else { v.muted = false; }
            }
        }
    }

    let block_size = 1024;
    let mut mix_buffer = vec![0.0; block_size * 2]; 
    let mut total_frames = 0;
    let max_frames = 44100 * 600; 

    loop {
        if voices.iter().all(|v| v.is_finished()) || total_frames > max_frames { break; }
        mix_buffer.fill(0.0);
        for v in &mut voices { v.add_to_mix(&mut mix_buffer, block_size)?; }

        if (manifest.master_gain - 1.0).abs() > 0.001 {
            for s in &mut mix_buffer { *s *= manifest.master_gain; }
        }

        for sample in &mix_buffer {
             let val = *sample;
             let soft_clipped = val.tanh(); 
             let s = (soft_clipped * i16::MAX as f32) as i16;
             writer.write_sample(s)?;
        }
        total_frames += block_size;
    }
    writer.finalize()?;
    Ok(())
}