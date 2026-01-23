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
        };
    }[];
    
    // Legacy support (optional, if AI returns single action)
    action?: string; 
    parameters?: any;

    message: string;
    confidence: number;
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
            content: m.content
        }));
        
        // 1. Normalize Context
        const context = JSON.stringify(tracks.map(t => ({ 
            id: t.id, 
            name: t.name.toLowerCase() 
        })));

        try {
            // 2. Call Backend
            const rawResponse = await invoke<string>('ask_ai', { 
                userInput, 
                trackContext: context,
                chatHistory: chatHistory // <--- Passing Memory 
            });

            const data: AIResponse = JSON.parse(rawResponse);
            console.log("🧠 AI Plan:", data);

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
                content: data.message,
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
             const mode = parameters?.mode === 'audio' ? 'record' : 'default';
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
        if (parameters?.track_id) {
            const exists = tracks.some(t => t.id === parameters.track_id);
            if (!exists) {
                console.warn("AI attempted action on missing track:", parameters.track_id);
                return;
            }
        }

        switch (action) {
            case 'set_gain':
                const gain = Math.max(0, Math.min(2.0, parameters.value ?? 1.0));
                await invoke('set_track_gain', { trackIndex: parameters.track_id - 1, gain });
                break;
            // --- NEW: Master Gain ---
            case 'set_master_gain':
                const masterGain = Math.max(0, Math.min(2.0, parameters.value ?? 1.0));
                await invoke('set_master_gain', { gain: masterGain });
                break;

            case 'set_pan':
                const pan = Math.max(-1, Math.min(1, parameters.value ?? 0));
                await invoke('set_track_pan', { trackIndex: parameters.track_id - 1, pan });
                break;
            case 'toggle_mute':
                await invoke('toggle_mute', { trackIndex: parameters.track_id - 1 });
                break;
            case 'toggle_solo':
                await invoke('toggle_solo', { trackIndex: parameters.track_id - 1 });
                break;
            case 'split_clip':
                if (parameters.time !== undefined) {
                    await invoke('split_clip', { trackIndex: parameters.track_id - 1, time: parameters.time });
                }
                break;
            case 'delete_track':
                 if (parameters.track_id) {
                    await invoke('delete_track', { trackIndex: parameters.track_id - 1 });
                 }
                 break;
            case 'undo': await invoke('undo'); break;
            case 'redo': await invoke('redo'); break;
        }

        window.dispatchEvent(new CustomEvent('refresh-project')); 
    }
}    

export const aiAgent = new AIAgent();