// haven/src/lib/managers/AIAgent.ts
import { invoke } from '@tauri-apps/api/core';

export type AIMessage = {
    role: 'user' | 'assistant';
    content: string;
    timestamp: number;
    action?: string;
};

interface AIResponse {
    // New: List of steps for multi-action commands
    steps?: {
        action: string;
        parameters?: {
            track_id?: number;
            value?: number;
            time?: number;
            mode?: string;
            direction?: string;
            count?: number;
            // --- ADD THESE LINES ---
            mute_original?: boolean;
            replace_original?: boolean;
            job_id?: string;
        };
    }[];
    
    // Legacy support (optional, if AI returns single action)
    action?: string; 
    parameters?: any;

    message: string;
    confidence: number;
}

// Helper to define the structure of Recording Status
interface RecordingState {
    is_recording: boolean;
    duration: number;
    current_rms: number;
    is_monitoring: boolean;
}

class AIAgent {
    // Reactive state using Svelte 5 Runes pattern if possible, 
    // but a simple class is safer for plain TS until integrated.
    // We will use a singleton pattern.
    
    async sendMessage(userInput: string, tracks: any[], previousMessages: AIMessage[] = []): Promise<AIMessage> {
        
        // 1. Prepare History for Backend
        // Filter out any failed/empty messages and map to {role, content}
        const chatHistory = previousMessages.map(m => ({
            role: m.role,
            content: m.content || " "
        }));

        // 2. FETCH REAL TIME STATE (Crucial Fix)
        // We need to know if Monitoring is ON so the AI knows whether to turn it OFF.
        let isMonitoring = false;
        try {
            const recState = await invoke<RecordingState>('get_recording_status');
            isMonitoring = recState.is_monitoring;
        } catch (e) {
            console.warn("Could not fetch recording status for AI context", e);
        }
        
        // --- ADDED: Fetch Real-time Telemetry (RMS & Peak) ---
        let liveMeters: any[] = [];
        try {
            liveMeters = await invoke('get_track_meters');
        } catch (e) {
            console.warn("Could not fetch AI telemetry", e);
        }
        
        // 1. Normalize Context (Keeping your clean JSON approach!)
        let context = JSON.stringify(tracks.map(t => {
            // Find telemetry for this specific track
            const telemetry = liveMeters.find(m => m.track_id === t.id);
            let rmsDb = -60.0;
            let peakDb = -60.0;

            if (telemetry) {
                const maxRms = Math.max(telemetry.rms_l, telemetry.rms_r);
                const maxPeak = Math.max(telemetry.peak_l, telemetry.peak_r);
                // Convert to Number to keep the JSON clean (no strings)
                rmsDb = maxRms > 0.00001 ? Number((20 * Math.log10(maxRms)).toFixed(1)) : -60.0;
                peakDb = maxPeak > 0.00001 ? Number((20 * Math.log10(maxPeak)).toFixed(1)) : -60.0;
            }
            return { 
                id: t.id, 
                name: t.name.toLowerCase(),
                gain: t.gain,
                pan: t.pan,
                muted: t.muted,
                solo: t.solo,
                monitoring: isMonitoring,
                telemetry_rms_db: rmsDb,   // <--- Added!
                telemetry_peak_db: peakDb  // <--- Added!
            };
        }));

        // Append the AI instructions to the JSON string so the AI knows how to use the new data
        context += `\n\nCRITICAL INSTRUCTIONS FOR AUDIO LEVELING:
            You now have 'telemetry_rms_db' (Perceived Loudness) and 'telemetry_peak_db' (True Peak) in the JSON context.
            - If the user asks to "balance the mix", "fix the levels", or says "it's too quiet", DO NOT GUESS.
            - A standard vocal RMS sits around -18dB to -12dB. If it is -30dB, it is objectively too quiet. Use 'set_track_gain' to increase it.
            - If 'telemetry_peak_db' is 0.0 dBFS or higher, the track is CLIPPING. You MUST reduce its 'set_track_gain' immediately.`;

        try {
            // 2. Call Backend
            const rawResponse = await invoke<string>('ask_ai', { 
                userInput, 
                trackContext: context,
                chatHistory: chatHistory // <--- Passing Memory 
            });

            const data: AIResponse = JSON.parse(rawResponse);
            console.log("🧠 AI Plan:", data);

            // --- FIX START: FORCE RECORD MODE IF USER ASKED FOR IT ---
            // If user mentioned "record" or "mic", but AI forgot to send 'mode: record', we inject it.
            const text = userInput.toLowerCase();
            if (text.includes('record') || text.includes('mic')) {
                // Helper to patch a step
                const patchStep = (step: any) => {
                    if (step.action === 'create_track') {
                        step.parameters = step.parameters || {};
                        // Only force it if AI didn't specify 'audio' or 'record' already
                        if (!step.parameters.mode) {
                            step.parameters.mode = 'record';
                            console.log("🔧 Auto-fixed: Injected 'mode: record' based on user input.");
                        }
                    }
                };

                // Patch both formats
                if (data.steps) data.steps.forEach(patchStep);
                else if (data.action) patchStep(data);
            }

            // CASE 1: New Multi-Step Format
            if (data.steps && Array.isArray(data.steps)) {
                for (const step of data.steps) {
                    await this.executeAction(step, tracks);
                    await new Promise(r => setTimeout(r, 100)); // Delay for UI safety
                }
            } 
            // CASE 2: Fallback (Old Single Action Format)
            else if (data.action) {
                 await this.executeAction({ 
                     action: data.action, 
                     parameters: data.parameters 
                 }, tracks);
            }

            return {
                role: 'assistant',
                content: data.message || "Done",
                timestamp: Date.now(),
                action: data.steps?.[0]?.action || data.action || 'none'
            };

        } catch (e) {
            console.error(e);
            return {
                role: 'assistant',
                content: "I'm having trouble connecting to the AI service.",
                timestamp: Date.now()
            };
        }
    }

    private async executeAction(step: any, tracks: any[]) {
        const { action, parameters } = step;
        console.log("🤖 AI Action:", action, parameters);

        if (!action || action === 'none' || action === 'clarify') return;

        // Transport & Recording (Handled by +page.svelte via Event)
        if (['play', 'pause', 'record', 'rewind', 'seek', 'toggle_monitor'].includes(action)) {
            window.dispatchEvent(new CustomEvent('ai-command', { detail: { 
                action,
                trackId: parameters?.track_id,
                time: parameters?.time,
                direction: parameters?.direction 
            } }));
            return;
        }

        // Create Track (Supports Count & Mode)
        if (action === 'create_track') {
            // const rawMode = parameters?.mode; 
            // const mode = (rawMode === 'record' || rawMode === 'audio') ? 'record' : 'default';
            const mode = 'record';
            const count = parameters?.count || 1; // Default to 1 if missing
            console.log(`🤖 Creating ${count} tracks (Mode: ${mode})`);
            // Loop X times to create multiple tracks
            for (let i = 0; i < count; i++) {
                window.dispatchEvent(new CustomEvent('ai-command', { detail: { action, mode } }));
                
                // Small delay to ensure order (optional but safer for UI)
                await new Promise(r => setTimeout(r, 50));
            }
            return;
        }

        // ... [Keep existing Safety Check & Switch for gain/pan/split etc.] ...
        // Safety Check: Track Existence
        // We verify the ID exists, but we NO LONGER convert it to an index.
        if (parameters?.track_id !== undefined) {
            const exists = tracks.some(t => t.id === parameters.track_id);
            if (!exists) {
                console.warn("AI attempted action on missing track:", parameters.track_id);
                return;
            }
        }

        switch (action) {

            case 'separate_stems':
                console.log("✂️ AI Separating Stems...");
                
                // 1. Determine Logic based on AI parameters
                const replaceOriginal = parameters.replace_original === true;
                const shouldMute = parameters.mute_original === true;

                // 2. Call Rust Backend
                // (If replacing, we don't need to mute, because we will delete it anyway)

                await invoke('separate_stems', { 
                    trackId: parameters.track_id,
                    muteOriginal: shouldMute, 
                    replaceOriginal: replaceOriginal
                });  
                break;

            case 'cancel_job':
                 if (parameters?.job_id) {
                     await invoke('cancel_ai_job', { jobId: parameters.job_id });
                 }
                 break;

            case 'set_gain':
                
                if (parameters.track_id !== undefined) {
                    let rawVal = parameters.value ?? 1.0;
                    
                    // PROMPT FIX UPDATE:
                    // The AI now correctly sends 0.0 - 2.0.
                    // We ONLY convert if it accidentally sends a percentage (e.g. 50, 80, 100).
                    if (rawVal > 2.0) {
                        rawVal = rawVal / 50; // Convert 100 -> 2.0
                    }
                    // Else: Use the value exactly as AI sent it (e.g. 1.0 is Unity, 2.0 is Max)

                    const gain = Math.max(0, Math.min(2.0, rawVal));
                    await invoke('set_track_gain', { 
                        trackId: parameters.track_id,
                        gain
                    });
                }
                break;    
            // --- NEW: Master Gain ---
            case 'set_master_gain':
                let rawMasterVal = parameters.value ?? 1.0;

                // PROMPT FIX UPDATE:
                // Same logic for Master Gain. Trust the AI unless value is huge.
                if (rawMasterVal > 2.0) {
                    rawMasterVal = rawMasterVal / 50;
                }

                const masterGain = Math.max(0, Math.min(2.0, rawMasterVal));
                await invoke('set_master_gain', { gain: masterGain });
                break;

            case 'set_pan':
                
                if (parameters.track_id !== undefined) {
                    const pan = Math.max(-1, Math.min(1, parameters.value ?? 0));
                    await invoke('set_track_pan', {
                        trackId: parameters.track_id,
                        pan 
                    });
                }
                break;
            case 'toggle_mute':
                
                if (parameters.track_id !== undefined){ 
                    await invoke('toggle_mute', { 
                        trackId: parameters.track_id 
                    });
                }    
                break;
            case 'toggle_solo':
                
                if (parameters.track_id !== undefined) {
                    await invoke('toggle_solo', { 
                        trackId: parameters.track_id 
                    });
                }    
                break;
            case 'split_clip':
               
                if (parameters.track_id !== undefined && parameters.time !== undefined) {
                    await invoke('split_clip', { 
                        trackId: parameters.track_id, 
                        time: parameters.time 
                    });
                }
                break;
            case 'delete_track':
                 if (parameters.track_id != undefined ) {
                    await invoke('delete_track', { 
                        trackId: parameters.track_id 
                    });
                 }
                 break;
            case 'update_eq':
                if (parameters.track_id !== undefined && parameters.band_index !== undefined) {
                    console.log("🎛️ AI updating EQ:", parameters);
                    await invoke('update_eq', {
                        args: {
                            track_id: parameters.track_id,
                            band_index: parameters.band_index,
                            filter_type: parameters.filter_type || "Peaking",
                            freq: parameters.freq || 1000.0,
                            q: parameters.q || 1.0,
                            gain: parameters.gain || 0.0,
                            active: true
                        }
                    });
                } else {
                    console.warn("AI sent update_eq without track_id or band_index");
                }
                break;

            case 'update_compressor':
                if (parameters.track_id !== undefined) {
                    console.log("🗜️ AI updating Compressor:", parameters);
                    await invoke('update_compressor', {
                        trackId: parameters.track_id,
                        params: {
                            is_active: true,
                            threshold_db: parameters.threshold_db ?? -20.0,
                            ratio: parameters.ratio ?? 4.0,
                            attack_ms: parameters.attack_ms ?? 5.0,
                            release_ms: parameters.release_ms ?? 50.0,
                            makeup_gain_db: parameters.makeup_gain_db ?? 0.0
                        }
                    });
                }
                break;     

            case 'undo': await invoke('undo'); break;
            case 'redo': await invoke('redo'); break;    
        }

        window.dispatchEvent(new CustomEvent('refresh-project')); 
    }
}    

export const aiAgent = new AIAgent();