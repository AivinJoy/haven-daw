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

    style?: string;     // "vocal", "master", "drums", "bass"
    intent?: string;    // "presence", "warmth", "clarity", "mud_cut", "air"
    space?: string;     // "room", "hall", "plate", "chamber"
    intensity?: number; // 0.0 to 1.0

    // Vocal Riding / Automation params (NEW)
    target_lufs?: number;
    noise_floor_db?: number;
    max_boost_db?: number;
    max_cut_db?: number;
    smoothness?: number;
    analysis_window_ms?: number;
    preserve_dynamics?: boolean;
}

export interface AIBatchRequest {
    version: string; // 🆕 Must be "1.0"
    commands: AICommand[];
    message: string;
    confidence: number;
    error?: string;
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
    "set_bpm", "set_gain", "set_pan", "toggle_mute","unmute", "toggle_solo", "unsolo", "move_clip", 
    "split_clip", "merge_clips", "delete_clip", "delete_track", "create_track", 
    // DSP & Automation
    "update_eq", "update_compressor", "update_reverb", 
    "clear_volume_automation", "duck_volume", "ride_vocal_level", "auto_gain_stage",
    "auto_compress", "auto_eq", "auto_reverb"
]);

const DSP_PRIORITY: Record<string, number> = {
    // 1️⃣ Input level correction FIRST
    "auto_gain_stage": 1,
    "set_gain": 2,

    // 2️⃣ Tone shaping
    "auto_eq": 3,
    "update_eq": 4,

    // 3️⃣ Dynamics control
    "auto_compress": 5,
    "update_compressor": 6,

    // 4️⃣ Automation (VERY IMPORTANT: before reverb)
    "clear_volume_automation": 7,
    "duck_volume": 8,
    "ride_vocal_level": 9,

    // 5️⃣ Time-based effects LAST
    "auto_reverb": 10,
    "update_reverb": 11
};
// ---------------------------------------------------------

class AIAgent {
    // 1. UPDATED SIGNATURE: Clean and lean.
    async sendMessage(
        userInput: string, 
        globalState: { bpm: number, timeSignature: string, playheadTime: number },
        previousMessages: AIMessage[] = [],
        selectedTrackId?: number
    ): Promise<AIMessage> {
        
        // STRICT TOKEN CONTROL
        const MAX_HISTORY = 4;
        const chatHistory = previousMessages.slice(-MAX_HISTORY).map(m => ({
            role: m.role,
            content: m.content || " "
        }));

        try {
            console.log("📤 Delegating intent and context building to Rust Engine...");
            
            // 2. THE FLIP: Send purely raw UI state to Rust. 
            // Rust will handle Intent Scoring, Track Filtering, and JSON Context Building!
            const rawResponse = await invoke<string>('ask_ai', { 
                userInput, 
                activeTrackId: selectedTrackId ?? null, // Safely pass null if undefined
                playheadTime: globalState.playheadTime,
                chatHistory
            });

            // 3. Parse the raw JSON from Rust/LLM
            const cleanResponse = rawResponse.replace(/```json\n?/g, '').replace(/```\n?/g, '').trim();
            console.log("🟢 RAW AI RESPONSE STRING:", cleanResponse);
            const data: AIBatchRequest = JSON.parse(cleanResponse);

            // --- 🚨 NEW: HANDLE AI CONFUSION ---
            if (data.error) {
                console.warn("AI aborted transaction:", data.error);
                return {
                    role: 'assistant',
                    content: "I couldn't complete that. Please select a specific track or provide more details.",
                    timestamp: Date.now(),
                    action: 'error'
                };
            }

            // --- SAFETY & SORTING INTERCEPTOR ---
            if (data.commands && Array.isArray(data.commands)) {
                let safeCommands = data.commands
                    .filter((cmd) => {
                        if (!ALLOWED_ACTIONS.has(cmd.action)) {
                            console.warn(`🚨 Blocked illegal/hallucinated AI command: ${cmd.action}`);
                            return false;
                        }
                        return true;
                    })
                    .map((cmd) => {
                        if (cmd.action === 'unmute') cmd.action = 'toggle_mute';
                        if (cmd.action === 'unsolo') cmd.action = 'toggle_solo';
                        
                        // 🛠️ HALLUCINATION SANITIZER & FALLBACKS
                        if (cmd.action === 'merge_clips') {
                            const anyCmd = cmd as any;
                            if (anyCmd.clip_id_1 !== undefined) {
                                console.warn("🔧 Sanitizing hallucinated clip IDs...");
                                cmd.clip_number = anyCmd.clip_id_1; // Take the first ID
                                delete anyCmd.clip_id_1;
                                delete anyCmd.clip_id_2;
                            }
                        }
                        if (cmd.action === 'split_clip') {
                            const anyCmd = cmd as any;
                            if (anyCmd.split_time !== undefined) {
                                console.warn("🔧 Sanitizing hallucinated split_time...");
                                cmd.time = anyCmd.split_time; // Fix the parameter name!
                                delete anyCmd.split_time;
                            }
                        }
                        if (cmd.action === 'update_eq') {
                            if (cmd.eq && Array.isArray(cmd.eq)) {
                                const eqParams = cmd.eq[0] || {};
                                cmd.band_index = eqParams.band_index ?? cmd.band_index;
                                cmd.filter_type = eqParams.filter_type ?? cmd.filter_type;
                                cmd.freq = eqParams.freq ?? cmd.freq;
                                cmd.q = eqParams.q ?? cmd.q;
                                cmd.gain = eqParams.gain ?? cmd.gain;
                                cmd.is_active = eqParams.active ?? eqParams.is_active ?? cmd.is_active;
                                delete cmd.eq; 
                            }
                            cmd.band_index = cmd.band_index ?? 0;
                            cmd.filter_type = cmd.filter_type ?? "Peaking";
                            cmd.freq = cmd.freq ?? 1000.0;
                            cmd.q = cmd.q ?? 1.0;
                            cmd.gain = cmd.gain ?? 0.0;
                            cmd.is_active = cmd.is_active ?? true;
                        } 
                        else if (cmd.action === 'update_compressor') {
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

            console.table(data.commands);
            
            // 5. DELEGATE ENTIRE BATCH TO RUST (Atomic Transaction)
            if (data.version === "1.0" && data.commands && data.commands.length > 0) {
                const transportCommands = ['play', 'pause', 'record', 'rewind', 'seek', 'toggle_monitor', 'separate_stems', 'undo', 'redo', 'create_track', 'set_bpm'];
                const dspCommands = data.commands.filter(c => !transportCommands.includes(c.action));
                const uiCommands = data.commands.filter(c => transportCommands.includes(c.action));

                console.log(`🔀 Routing: ${uiCommands.length} UI Commands, ${dspCommands.length} DSP Commands sent to Rust.`);

                uiCommands.forEach(cmd => {
                    window.dispatchEvent(new CustomEvent('ai-command', { detail: cmd }));
                });

                if (dspCommands.length > 0) {
                    try {
                        await invoke('execute_ai_transaction', { 
                            version: data.version,
                            commands: dspCommands 
                        });
                        
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