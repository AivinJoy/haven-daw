// src/decoder/mod.rs

pub mod control;
pub mod dsp;
pub mod output;
pub mod resample;
pub mod pipe;

use anyhow::anyhow;
use ringbuf::traits::Producer as RbProducer;
use rubato::Resampler; // for .reset()
use std::fs::File;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{channel, Receiver, Sender},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use symphonia::core::audio::{AudioBufferRef, SampleBuffer};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::default::{get_codecs, get_probe};

pub use control::DecoderCmd;

pub struct Decoder<P>
where
    P: RbProducer<Item = f32> + Send + 'static,
{
    path: String,
    producer: P,
    is_playing: Arc<AtomicBool>,
    output_channels: usize,
    source_sample_rate: u32,
    output_sample_rate: u32,
    cmd_rx: Receiver<DecoderCmd>,
    post_seek_fade_samples: usize,
}

impl<P> Decoder<P>
where
    P: RbProducer<Item = f32> + Send + 'static,
{
    pub fn new_with_ctrl(
        path: String,
        producer: P,
        is_playing: Arc<AtomicBool>,
        _source_channels: usize,
        output_channels: usize,
        source_sample_rate: u32,
        output_sample_rate: u32,
        cmd_rx: Receiver<DecoderCmd>,
    ) -> Self {
        Self {
            path,
            producer,
            is_playing,
            output_channels,
            source_sample_rate,
            output_sample_rate,
            cmd_rx,
            post_seek_fade_samples: 0,
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        thread::spawn(move || {
            if let Err(e) = self.run() {
                eprintln!("Decoder thread error: {e}");
            }
        })
    }

    // In src/decoder/mod.rs -> impl Decoder -> fn run

    fn run(mut self) -> Result<(), anyhow::Error> {
        let file = File::open(&self.path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let probed = get_probe().format(
            &Default::default(),
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )?;
        let mut format = probed.format;

        let track = format
            .default_track()
            .ok_or_else(|| anyhow!("no default audio track"))?;
        let track_id = track.id;

        let mut decoder = get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
        let mut sample_buf: Option<SampleBuffer<f32>> = None;
        let actual_rate = track.codec_params.sample_rate.unwrap_or(self.source_sample_rate);

        let mut resampler =
            resample::build_resampler(
                actual_rate,
                self.output_sample_rate,
                self.output_channels)?;
        let mut stage_planar: Vec<Vec<f32>> = vec![Vec::with_capacity(4096); self.output_channels];

        // Flag to track "End of File"
        let mut eof_reached = false;

        loop {
            // --- FIX 1: Handle Disconnects (Exit Thread) ---
            // Use a loop to process all pending commands
            loop {
                match self.cmd_rx.try_recv() {
                    Ok(cmd) => match cmd {
                        DecoderCmd::Seek(target) => {
                            let seconds = target.as_secs();
                            let frac = target.subsec_nanos() as f64 / 1_000_000_000f64;
                            let time = symphonia::core::units::Time::new(seconds, frac);
                            
                            // Try to seek
                            if let Err(e) = format.seek(
                                SeekMode::Accurate,
                                SeekTo::Time {
                                    time,
                                    track_id: Some(track_id),
                                },
                            ) {
                                eprintln!("Seek error: {}", e);
                            } else {
                                // Seek Success -> Reset EOF
                                eof_reached = false; 
                            }

                            // Clear buffers on seek
                            sample_buf = None;
                            for ch in &mut stage_planar { ch.clear(); }
                            if let Some(r) = &mut resampler { r.reset(); }
                            self.post_seek_fade_samples =
                                dsp::fade_samples_ms(self.output_sample_rate, 10) * self.output_channels;
                        }
                    },
                    // No more commands right now -> Break inner loop, continue decoding
                    Err(std::sync::mpsc::TryRecvError::Empty) => break, 
                    // Controller Disconnected -> APP CLOSED -> Return to exit thread
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => return Ok(()), 
                }
            }

            // 2. If at EOF, just wait.
            if eof_reached {
                thread::sleep(Duration::from_millis(10));
                continue;
            }

            // 3. Decode Next Packet
            let packet = match format.next_packet() {
                Ok(p) => p,
                Err(SymphoniaError::ResetRequired) => {
                    eof_reached = true; 
                    continue;
                }
                Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    eof_reached = true; // Mark EOF
                    continue; 
                }
                Err(_) => {
                    eof_reached = true; // Treat error as EOF
                    continue; 
                }
            };

            if packet.track_id() != track_id { continue; }

            match decoder.decode(&packet) {
                Ok(decoded) => {
                    let decoded_ch = decoded.spec().channels.count();

                    if sample_buf.is_none() {
                        let capacity = decoded.capacity() as u64;
                        sample_buf = Some(SampleBuffer::<f32>::new(capacity, *decoded.spec()));
                    }
                    let buf = sample_buf.as_mut().unwrap();

                    copy_interleaved_into_f32(buf, decoded);
                    let src_interleaved = buf.samples();

                    if resampler.is_some() {
                        if decoded_ch == self.output_channels {
                            dsp::append_interleaved_to_planar(src_interleaved, &mut stage_planar, self.output_channels);
                        } else {
                            let mixed = dsp::updown_mix_interleaved(src_interleaved, decoded_ch, self.output_channels);
                            dsp::append_interleaved_to_planar(&mixed, &mut stage_planar, self.output_channels);
                        }

                        while let Some(mut out_block) = resample::try_process_exact(resampler.as_mut().unwrap(), &mut stage_planar) {
                            let interleaved_out = dsp::interleave(out_block.as_mut_slice());
                            output::push_with_fade(&mut self.producer, &interleaved_out, &mut self.post_seek_fade_samples);
                        }
                    } else {
                        if decoded_ch == self.output_channels {
                            output::push_with_fade(&mut self.producer, src_interleaved, &mut self.post_seek_fade_samples);
                        } else {
                            let mixed = dsp::updown_mix_interleaved(src_interleaved, decoded_ch, self.output_channels);
                            output::push_with_fade(&mut self.producer, &mixed, &mut self.post_seek_fade_samples);
                        }
                    }
                }
                Err(SymphoniaError::IoError(_)) => continue,
                Err(SymphoniaError::DecodeError(_)) => continue,
                Err(_) => {
                    eof_reached = true;
                }
            }

            if !self.is_playing.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

pub fn spawn_decoder_with_ctrl<P>(
    path: String,
    producer: P,
    is_playing: Arc<AtomicBool>,
    source_channels: usize,
    output_channels: usize,
    source_sample_rate: u32,
    output_sample_rate: u32,
) -> (JoinHandle<()>, Sender<control::DecoderCmd>)
where
    P: RbProducer<Item = f32> + Send + 'static,
{
    let (tx, rx) = channel();
    let handle = Decoder::new_with_ctrl(
        path,
        producer,
        is_playing,
        source_channels,
        output_channels,
        source_sample_rate,
        output_sample_rate,
        rx,
    )
    .spawn();
    (handle, tx)
}

#[allow(dead_code)]
pub fn spawn_decoder<P>(
    path: String,
    producer: P,
    is_playing: Arc<AtomicBool>,
    source_channels: usize,
    output_channels: usize,
    source_sample_rate: u32,
    output_sample_rate: u32,
) -> JoinHandle<()>
where
    P: RbProducer<Item = f32> + Send + 'static,
{
    let (h, _tx) = spawn_decoder_with_ctrl(
        path,
        producer,
        is_playing,
        source_channels,
        output_channels,
        source_sample_rate,
        output_sample_rate,
    );
    h
}

#[inline]
fn copy_interleaved_into_f32(dst: &mut SampleBuffer<f32>, src: AudioBufferRef<'_>) {
    dst.copy_interleaved_ref(src);
}