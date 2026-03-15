// haven/src/lib/managers/AIAgent.ts
import { invoke } from '@tauri-apps/api/core';

export type AIMessage = {
    role: 'user' | 'assistant';
    content: string;
    timestamp: number;
    action?: string;
};

// 🆕 STRICT VERSIONED CONTRACT
export interface AICommand {
    action: string;
    track_id?: number;
    value?: number;
    time?: number;
    depth_db?: number;
    new_time?: number;
    clip_number?: number;
    bpm?: number;
    // EQ params
    band_index?: number;
    filter_type?: string;
    freq?: number;
    q?: number;
    gain?: number;
    // Compressor params
    threshold_db?: number;
    ratio?: number;
    attack_ms?: number;
    release_ms?: number;
    makeup_gain_db?: number;
}

export interface AIBatchRequest {
    version: string; // 🆕 Must be "1.0"
    commands: AICommand[];
    message: string;
    confidence: number;
}

interface RecordingState {
    is_recording: boolean;
    duration: number;
    current_rms: number;
    is_monitoring: boolean;
}

class AIAgent {
    // 1. UPDATED SIGNATURE: Accept globalState
    async sendMessage(
        userInput: string, 
        tracks: any[], 
        globalState: { bpm: number, timeSignature: string, playheadTime: number },
        previousMessages: AIMessage[] = []
    ): Promise<AIMessage> {
        
        const chatHistory = previousMessages.map(m => ({
            role: m.role,
            content: m.content || " "
        }));

        let isMonitoring = false;
        try {
            const recState = await invoke<RecordingState>('get_recording_status');
            isMonitoring = recState.is_monitoring;
        } catch (e) {
            console.warn("Could not fetch recording status", e);
        }
        
        let trackAnalysis: any[] = [];
        try { trackAnalysis = await invoke('get_track_analysis'); } catch (e) {}
        
        // 1. Provide Context (Now includes Project timing, Playhead, FX, and Colors)
        let context = JSON.stringify({
            project: {
                bpm: globalState.bpm,
                time_signature: globalState.timeSignature,
                playhead_position_seconds: globalState.playheadTime,
                monitoring_enabled: isMonitoring
            },
            tracks: tracks.map(t => {
                const data = trackAnalysis.find(a => a.track_id === t.id);
                const profile = data?.analysis; 
                
                return { 
                    id: t.id, 
                    name: t.name.toLowerCase(),
                    color: t.color, // <--- NOW THE AI KNOWS THE COLORS!
                    fader_linear: t.gain, // Note: Let Rust handle if this is linear or dB
                    pan: t.pan,
                    muted: t.muted,
                    solo: t.solo,
                    compressor: t.compressor, // <--- ADDED FX STATE
                    eq: t.eq,                 // <--- ADDED FX STATE
                    clips: t.clips?.map((c: any) => ({
                        clip_number: c.clipNumber ?? c.clip_number ?? 1,
                        start_time: Number((c.startTime ?? c.start_time ?? 0).toFixed(2)),
                        duration: Number(c.duration.toFixed(2))
                    })),
                    analysis: profile ? {
                        integrated_loudness_db: profile.integrated_loudness_db,
                        max_sample_peak_db: profile.max_sample_peak_db,
                        crest_factor_db: profile.crest_factor_db,
                        loudness_median_db: profile.loudness_p50_db, // P50 is the median
                        peak_events: profile.peak_events,            // Array of {t, db}
                        loud_windows: profile.loud_windows,          // Array of {t, db}
                        quiet_windows: profile.quiet_windows,        // Array of {t, db}
                        spectral_centroid_hz: profile.spectral_centroid_hz
                    } : "computing..."
                };
            })
        });

        // 2. STRICT JSON SCHEMA DEFINITION & AUTOMATION RULES
        context += `\n\nCRITICAL INSTRUCTIONS:
        You are an elite Audio DSP Engineer. 
        You MUST respond with a STRICT JSON payload matching the "1.0" API contract.
        Do NOT wrap the JSON in markdown blocks.
        
        RULES:
        1. 'depth_db' and 'target_lufs' MUST be in standard audio decibels (dB) or LUFS. 0.0 dB is unity gain.
        2. Never send percentages or linear gain.
        3. Allowed actions: play, pause, record, seek, set_bpm, set_gain, set_pan, toggle_mute, toggle_solo, move_clip, split_clip, merge_clips, delete_clip, delete_track, create_track, update_eq, update_compressor, clear_volume_automation, duck_volume, ride_vocal_level.

        AUTOMATION & VOCAL RIDING GUIDELINES:
        You have two different tools for volume control. Choose the correct one based on the user's request:

        TOOL A: Peak Protection (duck_volume)
        - Use this ONLY if the user explicitly asks to "fix clipping", "duck sudden peaks", or "remove plosives".
        - Analyze the track's 'analysis' object (peak_events). 
        - You MUST use the "duck_volume" command for individual peaks. 
        - Provide the exact peak time ('time') and the negative dB value to reduce the peak ('depth_db').
        - Example: {"action": "duck_volume", "track_id": 0, "time": 38.47, "depth_db": -2.9}

        TOOL B: Vocal Riding & Balancing (ride_vocal_level)
        - Use this if the user asks to "level the vocals", "balance the track", "make the vocal consistent", or "ride the volume".
        - This triggers an advanced offline DSP algorithm that handles everything automatically. Do NOT generate individual nodes.
        - You MUST provide the 'target_lufs' (default to -16.0 if not specified).
        - DYNAMIC NOISE GATE (Crucial): To prevent boosting background noise, you MUST analyze the 'quiet_windows' array in the track's 'analysis' object.
        - Find the average 'db' value of these quiet windows (this represents the room noise floor).
        - Set the 'noise_floor_db' parameter to be 3 to 5 dB HIGHER (closer to 0) than that noise floor average. 
        - Example: If 'quiet_windows' average around -29.0 dB, you must include "noise_floor_db": -25.0.
        - Example payload: {"action": "ride_vocal_level", "track_id": 0, "target_lufs": -16.0, "noise_floor_db": -25.0}
        - Optional parameters you can include: 'max_boost_db', 'max_cut_db', 'smoothness', 'analysis_window_ms'.`;

        try {
            console.log("📊 1. AI Context (Look at the peaks & windows here):", context);
            // 3. Let Backend AI Logic Handle LLM execution
            const rawResponse = await invoke<string>('ask_ai', { 
                userInput, 
                trackContext: context,
                chatHistory
            });

            // 4. Parse the raw JSON
            // Strip markdown blocks if the LLM hallucinated them
            const cleanResponse = rawResponse.replace(/```json\n?/g, '').replace(/```\n?/g, '').trim();
            const data: AIBatchRequest = JSON.parse(cleanResponse);

            // 5. DELEGATE ENTIRE BATCH TO RUST (Atomic Transaction)
            if (data.version === "1.0" && data.commands && data.commands.length > 0) {
                
                // We separate UI Transport commands from DSP commands
                const transportCommands = ['play', 'pause', 'record', 'rewind', 'seek', 'toggle_monitor', 'separate_stems'];
                const dspCommands = data.commands.filter(c => !transportCommands.includes(c.action));
                const uiCommands = data.commands.filter(c => transportCommands.includes(c.action));

                // A. Dispatch UI/Transport immediately
                uiCommands.forEach(cmd => {
                    window.dispatchEvent(new CustomEvent('ai-command', { detail: cmd }));
                });

                // B. Send ALL DSP commands to the Rust Engine for Validation & Execution
                if (dspCommands.length > 0) {
                    try {
                        // This single call will run through Layer 2, 3, and 4 in Rust!
                        await invoke('execute_ai_transaction', { 
                            version: data.version,
                            commands: dspCommands 
                        });
                        
                        // Buffer Delay: Give the Rust Audio threads a moment to sync the master state
                        // before the Svelte UI fetches the updated track data.
                        setTimeout(() => {
                            window.dispatchEvent(new CustomEvent('refresh-project')); 
                        }, 150);
                    } catch (transactionError) {
                        console.error("🛑 Rust Engine Rejected AI Transaction:", transactionError);
                        return {
                            role: 'assistant',
                            content: `I tried to do that, but the Audio Engine prevented it: ${transactionError}`,
                            timestamp: Date.now()
                        };
                    }
                }
            }

            return {
                role: 'assistant',
                content: data.message || "Done",
                timestamp: Date.now(),
                action: data.commands?.[0]?.action || 'none'
            };

        } catch (e) {
            console.error(e);
            return {
                role: 'assistant',
                content: "System communication error.",
                timestamp: Date.now()
            };
        }
    }
}    

export const aiAgent = new AIAgent();