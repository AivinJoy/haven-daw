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
    eq?: any[];
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
    "clear_volume_automation", "duck_volume", "ride_vocal_level", "auto_gain_stage"
]);

const DSP_PRIORITY: Record<string, number> = {
    "auto_gain_stage": 1,
    "set_gain": 2,
    "update_eq": 3,
    "update_compressor": 4,
    "update_reverb": 5,
    "clear_volume_automation": 6,
    "duck_volume": 7,
    "ride_vocal_level": 8
};
// ---------------------------------------------------------

class AIAgent {
    // 1. UPDATED SIGNATURE: Accept globalState
    async sendMessage(
        userInput: string, 
        tracks: any[], 
        globalState: { bpm: number, timeSignature: string, playheadTime: number },
        previousMessages: AIMessage[] = [],
        selectedTrackId?: number
    ): Promise<AIMessage> {
        
        const chatHistory = previousMessages.map(m => ({
            role: m.role,
            content: m.content || " "
        }));

        // Guarantee the AI sees newly imported tracks or stems instantly, 
        // bypassing any UI reactivity delays.
        let freshTracks = tracks;
        try {
            const projectState: any = await invoke('get_project_state');
            freshTracks = projectState.tracks;
            globalState.bpm = projectState.bpm;
            globalState.playheadTime = await invoke('get_position');
        } catch (e) {
            console.warn("Failed to sync fresh engine state, falling back to UI state:", e);
        }

        let isMonitoring = false;
        try {
            const recState = await invoke<RecordingState>('get_recording_status');
            isMonitoring = recState.is_monitoring;
        } catch (e) {
            console.warn("Could not fetch recording status", e);
        }

        // --- SMART CONTEXT FILTER ---
        // Only fetch heavy DSP analysis if the user is asking for mixing/mastering tasks
        const mixingKeywords = ['master', 'mix', 'level', 'ride', 'duck', 'eq', 'compressor', 'loudness', 'balance', 'vocal', 'peak', 'plosive', 'gain', 'automate', 'automation', 'stage', 'normalize', 'dynamics'];
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

        // --- NEW: MUTED TRACK FILTER ---
        const isUnmuteRequest = ['unmute', 'mute', 'listen', 'hear'].some(w => inputWords.includes(w));
        const activeTracks = tracks.filter(t => {
            if (!t.muted) return true; // Keep active tracks
            if (isGlobalRequest || isUnmuteRequest || selectedTrackId === t.id) return true; // Keep if explicitly targeted
            const trackNameTokens = t.name.toLowerCase().split(/[_\-\s]+/);
            return trackNameTokens.some((token: string) => inputWords.includes(token)) || inputWords.includes(t.id.toString());
        });

        // 2. BUILD CONTEXT WITH SMART FILTERING
        let context = JSON.stringify({
            project: {
                bpm: globalState.bpm,
                time_signature: globalState.timeSignature,
                playhead_position_seconds: globalState.playheadTime,
                monitoring_enabled: isMonitoring,
                target_track_id: selectedTrackId
            },
            tracks:freshTracks.map(t => {
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
                            loudness_p10_db: profile.loudness_p10_db,

                            // FIX: If the prompt relates to mixing/mastering, send ALL required arrays
                            ...(needsAnalysis ? {
                                max_sample_peak_db: profile.max_sample_peak_db,
                                crest_factor_db: profile.crest_factor_db,
                                loudness_median_db: profile.loudness_p50_db,
                                spectral_centroid_hz: profile.spectral_centroid_hz,
                                peak_events: profile.peak_events, // Required for duck_volume
                                loud_windows: profile.loud_windows,          
                                quiet_windows: profile.quiet_windows, // Required for ride_vocal_level
                            } : {
                                // LIGHTWEIGHT PAYLOAD: Normal transport/editing commands
                                max_sample_peak_db: profile.max_sample_peak_db,
                                _note: "Detailed arrays omitted. User did not request mixing."
                            })
                        } : {
                            status: "omitted",
                            message: "Explicitly mention this track's name or ID to retrieve full DSP analysis."
                        }
                    } : {})
                };
            })
        });

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

            // Determine resolved target track (Explicit UI selection OR Highest matched intent)
            let resolvedTargetId = selectedTrackId;
            if (resolvedTargetId === undefined) {
                const deduced = activeTracks.find(t => t.analysis?._match_reasons?.length > 0);
                if (deduced) resolvedTargetId = deduced.id;
            }

            // --- NEW SAFETY & SORTING INTERCEPTOR ---
            if (data.commands && Array.isArray(data.commands)) {
                let safeCommands = data.commands
                    .filter((cmd) => {
                        if (!ALLOWED_ACTIONS.has(cmd.action)) {
                            console.warn(`尅 Blocked illegal/hallucinated AI command: ${cmd.action}`);
                            return false;
                        }
                        return true;
                    })
                    .map((cmd) => {
                        // 🚀 TRACK ID LOCK ENFORCER
                        const transportActions = ['play', 'pause', 'record', 'rewind', 'seek', 'toggle_monitor', 'separate_stems'];
                        if (!isGlobalRequest && !transportActions.includes(cmd.action) && resolvedTargetId !== undefined) {
                            if (cmd.track_id !== undefined && cmd.track_id !== resolvedTargetId) {
                                console.warn(`🚨 Intercepted hallucinated track_id: ${cmd.track_id}. Overriding to ${resolvedTargetId}`);
                                cmd.track_id = resolvedTargetId; // Force correct ID
                            }
                        }

                        // 🛠️ HALLUCINATION SANITIZER & FALLBACKS
                        if (cmd.action === 'update_eq') {
                            // 1. Flatten if the AI nested it
                            if (cmd.eq && Array.isArray(cmd.eq)) {
                                console.warn("🔧 Sanitizing hallucinated nested EQ array from AI...");
                                const eqParams = cmd.eq[0] || {};
                                cmd.band_index = eqParams.band_index ?? cmd.band_index;
                                cmd.filter_type = eqParams.filter_type ?? cmd.filter_type;
                                cmd.freq = eqParams.freq ?? cmd.freq;
                                cmd.q = eqParams.q ?? cmd.q;
                                cmd.gain = eqParams.gain ?? cmd.gain;
                                cmd.is_active = eqParams.active ?? eqParams.is_active ?? cmd.is_active;
                                delete cmd.eq; 
                            }
                            
                            // 2. Guarantee Required Fields for Rust
                            cmd.band_index = cmd.band_index ?? 0;
                            cmd.filter_type = cmd.filter_type ?? "Peaking";
                            cmd.freq = cmd.freq ?? 1000.0;
                            cmd.q = cmd.q ?? 1.0;
                            cmd.gain = cmd.gain ?? 0.0;
                            cmd.is_active = cmd.is_active ?? true;
                        } 
                        else if (cmd.action === 'update_compressor') {
                            // Guarantee Required Fields for Rust Compressor
                            cmd.threshold_db = cmd.threshold_db ?? -18.0;
                            cmd.ratio = cmd.ratio ?? 4.0;
                            cmd.attack_ms = cmd.attack_ms ?? 10.0;
                            cmd.release_ms = cmd.release_ms ?? 100.0;
                            cmd.makeup_gain_db = cmd.makeup_gain_db ?? 0.0;
                            cmd.is_active = cmd.is_active ?? true;
                        }

                        return cmd;
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
                const transportCommands = ['play', 'pause', 'record', 'rewind', 'seek', 'toggle_monitor', 'separate_stems', 'undo', 'redo', 'create_track', 'set_bpm'];
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