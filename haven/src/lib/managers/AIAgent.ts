// haven/src/lib/managers/AIAgent.ts
import { invoke } from '@tauri-apps/api/core';

export type AIMessage = {
    role: 'user' | 'ai';
    content: string;
    timestamp: number;
    action?: string;
};

export type AIResponse = {
    action: string;
    parameters?: any;
    message: string;
    confidence?: number;
};

class AIAgent {
    // Reactive state using Svelte 5 Runes pattern if possible, 
    // but a simple class is safer for plain TS until integrated.
    // We will use a singleton pattern.
    
    async sendMessage(userInput: string, tracks: any[]): Promise<AIMessage> {
        // 1. Normalize Context
        const context = JSON.stringify(tracks.map(t => ({ 
            id: t.id, 
            name: t.name.toLowerCase() 
        })));

        try {
            // 2. Call Backend
            const rawResponse = await invoke<string>('ask_ai', { 
                userInput, 
                trackContext: context 
            });

            // 3. Parse JSON
            const data: AIResponse = JSON.parse(rawResponse);

            // 4. Execute Action (The Guard)
            await this.executeAction(data, tracks);

            return {
                role: 'ai',
                content: data.message,
                timestamp: Date.now(),
                action: data.action
            };

        } catch (e) {
            console.error(e);
            return {
                role: 'ai',
                content: "I'm having trouble connecting to the AI service.",
                timestamp: Date.now()
            };
        }
    }

    private async executeAction(response: AIResponse, tracks: any[]) {
        const { action, parameters } = response;
        console.log("🤖 AI Action:", action, parameters);

        if (!action || action === 'none' || action === 'clarify') return;

        // Safety Check: Track Existence
        if (parameters?.track_id) {
            const exists = tracks.some(t => t.id === parameters.track_id);
            if (!exists) {
                console.warn("AI attempted action on missing track:", parameters.track_id);
                return;
            }
        }

        // Action Dispatcher
        switch (action) {
            case 'set_gain':
                // Clamp Value 0.0 to 1.5
                const gain = Math.max(0, Math.min(1.5, parameters.value ?? 1.0));
                // Convert 1-based ID to 0-based index if needed, or backend handles it.
                // Our backend 'set_track_gain' usually expects index. 
                // Let's assume Track ID 1 = Index 0.
                await invoke('set_track_gain', { trackIndex: parameters.track_id - 1, gain });
                break;

            case 'set_pan':
                // Clamp Value -1.0 to 1.0
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
            
            case 'undo':
                await invoke('undo');
                break;
                
            case 'create_track':
                 await invoke('create_track');
                 break;

            case 'delete_track':
                 if (parameters.track_id) {
                    await invoke('delete_track', { trackIndex: parameters.track_id - 1 });
                 }
                 break;    
        }

        // 🚀 CRITICAL FIX: Tell the UI to refresh its state from the Backend
        // This makes the mute buttons, knobs, and timeline update instantly.
        window.dispatchEvent(new CustomEvent('refresh-project'));
    }
}

export const aiAgent = new AIAgent();