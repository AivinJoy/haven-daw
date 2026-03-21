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
    // Reverb params (NEW)
    is_active?: boolean;
    room_size?: number;
    damping?: number;
    pre_delay_ms?: number;
    mix?: number;
    width?: number;
    low_cut_hz?: number;
    high_cut_hz?: number;

    // Vocal Riding / Automation params (NEW)
    target_lufs?: number;
    noise_floor_db?: number;
    max_boost_db?: number;
    max_cut_db?: number;
    smoothness?: number;
    analysis_window_ms?: number;
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

// --- DAW SAFETY LAYER: Allowed Actions & DSP Priority ---
const ALLOWED_ACTIONS = new Set([
    // UI / Transport Commands
    "play", "pause", "record", "rewind", "seek", "toggle_monitor", "separate_stems",
    // Track / Clip Management
    "set_bpm", "set_gain", "set_pan", "toggle_mute", "toggle_solo", "move_clip", 
    "split_clip", "merge_clips", "delete_clip", "delete_track", "create_track", 
    // DSP & Automation
    "update_eq", "update_compressor", "update_reverb", 
    "clear_volume_automation", "duck_volume", "ride_vocal_level"
]);

const DSP_PRIORITY: Record<string, number> = {
    "set_gain": 1,
    "update_eq": 2,
    "update_compressor": 3,
    "update_reverb": 4,
    "clear_volume_automation": 5,
    "duck_volume": 6,
    "ride_vocal_level": 7
};
// ---------------------------------------------------------

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

        // --- SMART CONTEXT FILTER ---
        // Only fetch heavy DSP analysis if the user is asking for mixing/mastering tasks
        const mixingKeywords = ['master', 'mix', 'level', 'ride', 'duck', 'eq', 'compressor', 'loudness', 'balance', 'vocal', 'peak', 'plosive'];
        const needsAnalysis = mixingKeywords.some(kw => userInput.toLowerCase().includes(kw));
        
        let trackAnalysis: any[] = [];
        if (needsAnalysis) {
            try { trackAnalysis = await invoke('get_track_analysis'); } catch (e) {}
        }
        
        // 1. INTENT DICTIONARIES
        const trackAliases: Record<string, string[]> = {
            "vocal": ["vox", "vocals", "voice", "lead", "singer", "singing"],
            "guitar": ["guitars", "acoustic", "electric", "strum", "riff"],
            "bass": ["808", "sub", "bassline"],
            "drum": ["drums", "kick", "snare", "hihat", "percussion", "beat"]
        };

        const actionIntents: Record<string, string[]> = {
            "peaks": ["peak", "clip", "clipping", "plosive", "pop", "duck", "distort", "harsh"],
            "dynamics": ["ride", "level", "balance", "quiet", "loud", "compress", "compressor", "dynamic", "volume"],
            "eq": ["eq", "bright", "muddy", "dark", "thin", "frequency", "tone"]
        };

        // NEW: Global Context Awareness
        const globalIntents = ["master", "mix", "overall", "final", "entire", "everything", "all"];

        const lowercaseInput = userInput.toLowerCase();
        const inputWords = lowercaseInput.replace(/[.,!?]/g, '').split(/\s+/);

        // PRECOMPUTE: Actions & Global Scope
        const activeActions = new Set<string>();
        for (const [action, keywords] of Object.entries(actionIntents)) {
            if (keywords.some((kw: string) => inputWords.includes(kw))) {
                activeActions.add(action);
            }
        }
        
        const isGlobalRequest = globalIntents.some((term: string) => inputWords.includes(term));
        const intentConfidence = activeActions.size;

        // 2. BUILD CONTEXT WITH SMART FILTERING
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
                
                const trackNameLower = t.name.toLowerCase();
                const trackNameTokens = trackNameLower.split(/[_\-\s]+/);
                const trackIdStr = t.id.toString();

                // 3. MULTI-TRACK INTENT TRACKING (With Scoring)
                const matchedReasons: string[] = [];

                // A: Exact ID match
                if (inputWords.includes(trackIdStr)) {
                    matchedReasons.push("id");
                }

                // B: Strict Full Name Match (All tokens must match)
                if (trackNameTokens.length > 0 && trackNameTokens.every((token: string) => inputWords.includes(token))) {
                    matchedReasons.push("name_full");
                } else if (trackNameTokens.some((token: string) => inputWords.includes(token))) {
                    matchedReasons.push("name_token");
                }

                // C: Weighted Alias Match
                for (const [category, aliases] of Object.entries(trackAliases)) {
                    if (trackNameTokens.includes(category) || trackNameTokens.some((token: string) => aliases.includes(token))) {
                        let aliasScore = 0;
                        if (inputWords.includes(category)) aliasScore += 2;
                        if (aliases.some((alias: string) => inputWords.includes(alias))) aliasScore += 1;

                        if (aliasScore > 0) {
                            matchedReasons.push(`alias_${category}_${aliasScore}`);
                        }
                    }
                }

                // Target Track Logic now respects Global Intents
                const isTargetTrack = isGlobalRequest || matchedReasons.length > 0;
                
                return { 
                    id: t.id, 
                    name: trackNameLower,
                    color: t.color, 
                    fader_linear: t.gain, 
                    pan: t.pan,
                    muted: t.muted,
                    solo: t.solo,
                    compressor: t.compressor, 
                    eq: t.eq,                 
                    clips: t.clips?.map((c: any) => ({
                        clip_number: c.clipNumber ?? c.clip_number ?? 1,
                        start_time: Number((c.startTime ?? c.start_time ?? 0).toFixed(2)),
                        duration: Number(c.duration.toFixed(2))
                    })),
                    
                    // 4. STRUCTURED OMISSION & PAYLOAD TRIMMING
                    ...(profile ? {
                        analysis: isTargetTrack ? {
                            _match_reasons: matchedReasons, 
                            _active_actions: Array.from(activeActions),
                            _is_global: isGlobalRequest,
                            
                            // Always include basic scalar numbers
                            integrated_loudness_db: profile.integrated_loudness_db,

                            // The "Fix This" Fallback: If 0 confidence, send ONLY scalars, NO arrays
                            ...(intentConfidence === 0 && !isGlobalRequest ? {
                                max_sample_peak_db: profile.max_sample_peak_db,
                                crest_factor_db: profile.crest_factor_db,
                                loudness_median_db: profile.loudness_p50_db,
                                spectral_centroid_hz: profile.spectral_centroid_hz,
                                _note: "Detailed arrays omitted due to low intent confidence."
                            } : {
                                // High Confidence OR Global Request: Send targeted arrays
                                ...(activeActions.has("peaks") || isGlobalRequest ? {
                                    max_sample_peak_db: profile.max_sample_peak_db,
                                    peak_events: profile.peak_events,
                                } : {}),

                                ...(activeActions.has("dynamics") || isGlobalRequest ? {
                                    crest_factor_db: profile.crest_factor_db,
                                    loudness_median_db: profile.loudness_p50_db,
                                    loud_windows: profile.loud_windows,          
                                    quiet_windows: profile.quiet_windows, 
                                } : {}),

                                ...(activeActions.has("eq") || isGlobalRequest ? {
                                    spectral_centroid_hz: profile.spectral_centroid_hz
                                } : {})
                            })
                        } : {
                            status: "omitted",
                            message: "Explicitly mention this track's name or ID to retrieve full DSP analysis."
                        }
                    } : {})
                };
            })
        });

        // 2. STRICT JSON SCHEMA DEFINITION & AUTOMATION RULES
        context += `\n\nCRITICAL INSTRUCTIONS:
        You are an elite Audio DSP Engineer. 
        You MUST respond with a STRICT JSON payload matching the "1.0" API contract.
        Do NOT wrap the JSON in markdown blocks.
        
        RULES:
        1. 'depth_db' and 'target_lufs' MUST be in standard audio decibels (dB) or LUFS. 
        2. GAIN EXCEPTION: The 'set_gain' and 'set_pan' commands strictly use LINEAR values. For 'set_gain', 0.0 is silence, 1.0 is unity gain, and 2.0 is +6dB. If you want to drop the gain for headroom, send a linear value like 0.5. DO NOT SEND DECIBELS FOR SET_GAIN.
        3. Allowed actions: play, pause, record, rewind, seek, set_bpm, set_gain, set_pan, toggle_mute, toggle_solo, move_clip, split_clip, merge_clips, delete_clip, delete_track, create_track, update_eq, update_compressor, update_reverb, clear_volume_automation, duck_volume, ride_vocal_level.
        4. GAIN STAGING IS MANDATORY: Before applying any heavy compression or EQ boosts, ALWAYS prepend a "set_gain" command with a linear value of 0.5 to provide headroom.
        5. VOCAL CHAINS: If asked to process a vocal, you MUST include 'update_reverb' in your chain to provide spatial depth, unless explicitly told to keep it dry.


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
            console.log("🟢 RAW AI RESPONSE STRING:", cleanResponse);
            const data: AIBatchRequest = JSON.parse(cleanResponse);

            // --- NEW SAFETY & SORTING INTERCEPTOR ---
            if (data.commands && Array.isArray(data.commands)) {
                let safeCommands = data.commands
                    .filter((cmd) => {
                        if (!ALLOWED_ACTIONS.has(cmd.action)) {
                            console.warn(`🛑 Blocked illegal/hallucinated AI command: ${cmd.action}`);
                            return false;
                        }
                        return true;
                    })
                    .sort((a, b) => {
                        const priorityA = DSP_PRIORITY[a.action] || 99;
                        const priorityB = DSP_PRIORITY[b.action] || 99;
                        return priorityA - priorityB;
                    });

                // --- PHASE 4: COMMAND EXPANSION & GAIN STAGING ---
                let expandedCommands: any[] = [];
                for (let cmd of safeCommands) {
                    // 1. Auto-Clear old automation before riding new automation
                    if (cmd.action === 'ride_vocal_level') {
                        expandedCommands.push({ 
                            action: 'clear_volume_automation', 
                            track_id: cmd.track_id 
                        });
                    }
                    expandedCommands.push(cmd);
                }
                
                data.commands = expandedCommands;
            }
            // ----------------------------------------
            // ----------------------------------------

            console.table(data.commands);
            // 5. DELEGATE ENTIRE BATCH TO RUST (Atomic Transaction)
            if (data.version === "1.0" && data.commands && data.commands.length > 0) {
                
                // We separate UI Transport commands from DSP commands
                const transportCommands = ['play', 'pause', 'record', 'rewind', 'seek', 'toggle_monitor', 'separate_stems'];
                const dspCommands = data.commands.filter(c => !transportCommands.includes(c.action));
                const uiCommands = data.commands.filter(c => transportCommands.includes(c.action));

                console.log(`🔀 Routing: ${uiCommands.length} UI Commands, ${dspCommands.length} DSP Commands sent to Rust.`);

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