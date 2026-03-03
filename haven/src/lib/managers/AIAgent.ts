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
    clip_number?: number;
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
    async sendMessage(userInput: string, tracks: any[], previousMessages: AIMessage[] = []): Promise<AIMessage> {
        
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
        
        // 1. Provide Context (No Math, Just Facts)
        let context = JSON.stringify(tracks.map(t => {
            const data = trackAnalysis.find(a => a.track_id === t.id);
            const profile = data?.analysis; 
            
            return { 
                id: t.id, 
                name: t.name.toLowerCase(),
                gain_db: t.gain, // Note: Let Rust handle if this is linear or dB
                pan: t.pan,
                muted: t.muted,
                solo: t.solo,
                clips: t.clips?.map((c: any) => ({
                    clip_number: c.clipNumber,
                    start_time: Number(c.startTime.toFixed(2)),
                    duration: Number(c.duration.toFixed(2))
                })),
                analysis: profile ? {
                    integrated_rms_db: Number(profile.integrated_rms_db.toFixed(1)),
                    max_sample_peak_db: Number(profile.max_sample_peak_db.toFixed(1)),
                    crest_factor_db: Number(profile.crest_factor_db.toFixed(1)),
                    spectral_centroid_hz: Math.round(profile.spectral_centroid_hz)
                } : "computing..."
            };
        }));

        // 2. 🆕 STRICT JSON SCHEMA DEFINITION
        context += `\n\nCRITICAL INSTRUCTIONS:
        You are an elite Audio DSP Engineer. 
        You MUST respond with a STRICT JSON payload matching the "1.0" API contract.
        Do NOT wrap the JSON in markdown blocks.
        
        {
          "version": "1.0",
          "commands": [
            { "action": "set_gain", "track_id": 0, "value": -3.0 }
          ],
          "message": "I reduced the gain to prevent clipping.",
          "confidence": 0.95
        }
        
        RULES:
        1. 'value' for gain/pan/etc MUST be in standard audio units (dB for gain, -1.0 to 1.0 for pan).
        2. Never send percentages.
        3. Only use allowed actions: play, pause, record, set_gain, set_pan, toggle_mute, toggle_solo, split_clip, merge_clips, delete_clip, update_eq, update_compressor.`;

        try {
            // 3. Let Backend AI Logic Handle LLM execution
            const rawResponse = await invoke<string>('ask_ai', { 
                userInput, 
                trackContext: context,
                chatHistory
            });

            // 4. Parse the raw JSON
            const data: AIBatchRequest = JSON.parse(rawResponse);
            console.log("🧠 AI Intent:", data);

            // 5. 🆕 DELEGATE ENTIRE BATCH TO RUST (Atomic Transaction)
            // No more frontend looping. No more math. No more default injections.
            if (data.version === "1.0" && data.commands && data.commands.length > 0) {
                
                // We separate UI Transport commands from DSP commands
                const transportCommands = ['play', 'pause', 'record', 'rewind', 'seek', 'toggle_monitor'];
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
                        
                        // If successful, refresh the UI
                        window.dispatchEvent(new CustomEvent('refresh-project')); 
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