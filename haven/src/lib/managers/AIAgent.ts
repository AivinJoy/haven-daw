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
            mute_original?: boolean;
            replace_original?: boolean;
            job_id?: string;
            clip_number?: number; // <--- NEW: Allow AI to target clips
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
        
        // --- NEW: Fetch Integrated Offline Track Analysis ---
        let trackAnalysis: any[] = [];
        try {
            trackAnalysis = await invoke('get_track_analysis');
        } catch (e) {
            console.warn("Could not fetch integrated AI track analysis", e);
        }
        
        // 1. Normalize Context (Clean JSON approach)
        let context = JSON.stringify(tracks.map(t => {
            // Find the analysis profile for this track
            const data = trackAnalysis.find(a => a.track_id === t.id);
            const profile = data?.analysis; // Will be null if still computing
            
            return { 
                id: t.id, 
                name: t.name.toLowerCase(),
                gain: t.gain,
                pan: t.pan,
                muted: t.muted,
                solo: t.solo,
                monitoring: isMonitoring,
                clips: t.clips?.map((c: any) => ({
                    clip_number: c.clipNumber,
                    start_time: Number(c.startTime.toFixed(2)),
                    duration: Number(c.duration.toFixed(2))
                })), // <--- NEW: AI can now "see" the clips!
                // AI gets the true statistical average of the track
                analysis: profile ? {
                    integrated_rms_db: Number(profile.integrated_rms_db.toFixed(1)),
                    max_sample_peak_db: Number(profile.max_sample_peak_db.toFixed(1)),
                    crest_factor_db: Number(profile.crest_factor_db.toFixed(1)),
                    spectral_centroid_hz: Math.round(profile.spectral_centroid_hz),
                    energy_lows_pct: Number(profile.energy_lows_pct.toFixed(2)),
                    energy_mids_pct: Number(profile.energy_mids_pct.toFixed(2)),
                    energy_highs_pct: Number(profile.energy_highs_pct.toFixed(2))
                } : "computing..."
            };
        }));

        // Append Advanced AI Engineering Instructions
        context += `\n\nCRITICAL INSTRUCTIONS FOR AUDIO ENGINEERING:
            You now have a professional 'analysis' profile for each track. Use it to make mixing decisions:
            
            1. LEVELING (RMS & PEAK):
               - 'integrated_rms_db' is perceived loudness. Target -18dB to -14dB. If it's -30dB, it is objectively too quiet; use 'set_gain' > 1.0.
               - If 'max_sample_peak_db' is near 0.0 dBFS, it is CLIPPING. Reduce 'set_gain'.

            2. COMPRESSION (CREST FACTOR):
               - 'crest_factor_db' (Peak minus RMS) reveals dynamics. 
               - HIGH Crest Factor (>12dB): The track is "Transient" (e.g., Drums, Percussion, Plucks). Use fast attack (1-5ms), fast release (50ms), ratio 4:1 to 8:1.
               - LOW Crest Factor (<6dB): The track is "Sustained" (e.g., Synth Pads, distorted guitars, heavy bass). Needs little to no compression, or very slow attack (30ms+), low ratio (2:1).

            3. EQ (SPECTRAL CENTROID & ENERGY):
               - Look at 'energy_lows_pct', 'mids', and 'highs' to identify the instrument.
               - High 'energy_lows_pct' (>0.5) = Bass/Kick. Boost lows, cut highs.
               - High 'energy_mids_pct' = Vocals/Guitars. To increase presence, boost 'Peaking' at freq 2000-4000Hz. To remove "mud", cut at freq 250-400Hz.
               - If a track is clashing, use EQ to carve out space based on their centroids.`;

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
        let { action, parameters } = step;

        // --- SAFEGUARDS against AI hallucinations ---
        if (action === 'unsolo') action = 'toggle_solo';
        if (action === 'unmute') action = 'toggle_mute';
        
        // Ensure parameters object exists
        if (!parameters) parameters = {};

        // UI SAFEGUARD: Default track_id to 0 if the user/AI didn't specify one
        if (parameters.track_id === undefined || parameters.track_id === null) {
            parameters.track_id = 0;
        }

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
                    let rawPan = parameters.value;

                    // UI SAFEGUARD 1: If AI forgot the value entirely, assume Center
                    if (rawPan === undefined || rawPan === null) {
                        rawPan = 0.0;
                    }

                    // UI SAFEGUARD 2: If the AI hallucinates a string instead of a number
                    if (typeof rawPan === 'string') {
                        const strVal = (rawPan as string).toLowerCase();
                        if (strVal.includes('left')) rawPan = -1.0;
                        else if (strVal.includes('right')) rawPan = 1.0;
                        else rawPan = 0.0; // Center fallback
                    }
                    
                    // UI SAFEGUARD 3: If AI uses percentages (100 instead of 1.0)
                    if (rawPan > 1.0 || rawPan < -1.0) {
                        rawPan = rawPan / 100.0; 
                    }
                    
                    // Final Clamp to ensure UI Knob never glitches out of bounds
                    const pan = Math.max(-1.0, Math.min(1.0, rawPan));
                    
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
                console.log("🔥 AI Split Clip. Params:", parameters);
                // Default to track 0 if the AI forgot
                const splitTrackId = parameters.track_id ?? 0;
                
                if (parameters.time !== undefined) {
                    await invoke('split_clip', { 
                        trackId: splitTrackId, 
                        time: parameters.time 
                    });
                } else {
                    console.error("⚠️ AI forgot the time parameter for splitting!");
                }
                break;

            case 'merge_clips':
                console.log("🔥 AI Merge Clips. Params:", parameters);
                // AI often uses 'value' by mistake instead of 'clip_number'
                let mergeClipNum = parameters.clip_number ?? parameters.value; 
                
                if (mergeClipNum === undefined) {
                    console.warn("⚠️ AI forgot clip number! Defaulting to 1.");
                    mergeClipNum = 1;
                }
                
                await invoke('merge_clip_with_next', { 
                    trackId: parameters.track_id ?? 0, 
                    clipIndex: mergeClipNum - 1 // Convert 1-based UI to 0-based Backend
                });
                break;
            case 'delete_clip':
                console.log("🔥 AI Delete Clip. Params:", parameters);
                // Catch the AI's missing parameters
                let delClipNum = parameters.clip_number ?? parameters.value;
                
                if (delClipNum === undefined) {
                    console.warn("⚠️ AI forgot clip number! Defaulting to 1.");
                    delClipNum = 1;
                }
                
                await invoke('delete_clip', { 
                    trackId: parameters.track_id ?? 0, 
                    clipIndex: delClipNum - 1 
                });
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

        // --- FIX: UI RACE CONDITION ---
        // Give the Rust lock-free audio thread 50ms to actually process 
        // the pan/gain/mute commands before we fetch the updated state.
        await new Promise(resolve => setTimeout(resolve, 50));

        window.dispatchEvent(new CustomEvent('refresh-project')); 
    }
}    

export const aiAgent = new AIAgent();