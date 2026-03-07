// src/engine/time.rs

use std::time::Duration;
use serde::{Serialize, Deserialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct TimeSignature {
    pub numerator: u32,   // e.g., 4
    pub denominator: u32, // e.g., 4
}

impl Default for TimeSignature {
    fn default() -> Self {
        Self { numerator: 4, denominator: 4 }
    }
}

/// NEW: Holds data for a single grid line on the timeline.
#[derive(Debug, Clone, Serialize)]
pub struct GridLine {
    /// The exact time in seconds corresponding to this line.
    pub time: f64,
    /// Is this line the very start of a bar? (e.g., 1.1.0, 2.1.0)
    pub is_bar_start: bool,
    /// The human-readable bar number (1-indexed).
    pub bar_number: u32,
}


/// The "Brain" that relates Real Time (Seconds) to Musical Time (Bars/Beats).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TempoMap {
    pub bpm: f64,
    pub signature: TimeSignature,
}

impl Default for TempoMap {
    fn default() -> Self {
        Self {
            bpm: 120.0,
            signature: TimeSignature::default(),
        }
    }
}

impl TempoMap {
    pub fn new(bpm: f64, numerator: u32, denominator: u32) -> Self {
        Self {
            bpm,
            signature: TimeSignature { numerator, denominator },
        }
    }

    /// 1. MECHANICAL CLOCK: The universal clock unit. 
    /// In a professional DAW, BPM ALWAYS defines the length of a Quarter Note.
    pub fn seconds_per_quarter_note(&self) -> f64 {
        60.0 / self.bpm
    }

    /// 2. MUSICAL BEAT CLOCK: The duration of a single musical beat, defined by the denominator.
    /// In 4/4, this is a quarter note. In 6/8, this is an eighth note.
    pub fn seconds_per_musical_beat(&self) -> f64 {
        self.seconds_per_quarter_note() * (4.0 / self.signature.denominator as f64)
    }

    /// Seconds per bar is the number of musical beats (numerator) times the length of each beat.
    pub fn seconds_per_bar(&self) -> f64 {
        self.seconds_per_musical_beat() * self.signature.numerator as f64
    }

    /// Convert exact Duration to a Bar/Beat representation for the UI Transport.
    /// Returns (bar, beat, percentage_of_beat)
    pub fn timestamp_to_musical(&self, position: Duration) -> (u32, u32, f64) {
        let total_seconds = position.as_secs_f64();
        let seconds_per_beat = self.seconds_per_musical_beat();
        
        let total_beats = total_seconds / seconds_per_beat;
        let beats_per_bar = self.signature.numerator as f64;

        let bar_index = (total_beats / beats_per_bar).floor();
        let beat_in_bar = total_beats % beats_per_bar;
        
        // Bars are usually 1-indexed for humans, but 0-indexed for math.
        // We return 1-indexed Bars (1, 2, 3...) and 1-indexed Beats.
        (
            bar_index as u32 + 1, 
            beat_in_bar.floor() as u32 + 1, 
            beat_in_bar.fract()
        )
    }

    /// Generates grid lines (in Seconds) for a specific time range.
    /// This is what the Frontend will ask for to draw the grid.
    /// `resolution`: 1 = bars, 4 = quarter notes, 8 = eighth notes, 16 = sixteenths
    pub fn get_grid_lines(&self, start: Duration, end: Duration, resolution: u32) -> Vec<GridLine> {
        let spq = self.seconds_per_quarter_note();
        let quarters_per_bar = self.signature.numerator as f64 * (4.0 / self.signature.denominator as f64);
        
        // Grid Resolution strictly follows standard note divisions (1=bar, 4=quarter, 8=eighth)
        let quarters_per_step = if resolution == 1 {
            quarters_per_bar
        } else {
            4.0 / resolution as f64
        };

        let seconds_per_step = quarters_per_step * spq;
        
        let start_sec = start.as_secs_f64();
        let end_sec = end.as_secs_f64();

        // 1. Calculate the starting STEP INDEX (Integer)
        // This aligns us perfectly to the grid, regardless of scroll position
        let mut step_index = (start_sec / seconds_per_step).ceil() as u64;
        let mut lines = Vec::new();

        // Calculate once outside the loop
        let steps_per_bar = (quarters_per_bar / quarters_per_step).round() as u64;

        // 2. Loop by Integer Steps (No float accumulation drift)
        loop {
            let time = step_index as f64 * seconds_per_step;
            if time > end_sec + 0.001 {
                break;
            }

            // 3. Calculate Bar/Beat Logic
            let is_bar_start = if steps_per_bar == 0 {
                true 
            } else {
                step_index % steps_per_bar == 0
            };

            // Calculate Bar Number (1-indexed)
            let bar_number = if steps_per_bar == 0 {
                step_index as u32 + 1
            } else {
                (step_index / steps_per_bar) as u32 + 1
            };

            lines.push(GridLine {
                time,
                is_bar_start,
                bar_number,
            });

            step_index += 1;
        }
        
        lines
    }
}