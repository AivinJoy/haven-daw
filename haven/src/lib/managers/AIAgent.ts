// haven/src/lib/managers/AIAgent.ts
import { invoke } from '@tauri-apps/api/core';

export type AIMessage = {
    role: 'user' | 'assistant';
    content: string;
    timestamp: number;
    action?: string;
};

// 🆕 We can loosen this since Rust does the strict validation now
export interface AICommand {
    action: string;
    [key: string]: any; 
}

// 🆕 Matches the Rust AIExecutionTrace struct exactly
export interface AIExecutionTrace {
    raw_response: string;
    message?: string;
    parsed_actions: AICommand[];
    normalized_actions: AICommand[];
    execution_order: AICommand[];
    errors: string[];
}

class AIAgent {
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
            
            // 1. Send purely raw UI state to Rust. 
            const rawResponse = await invoke<string>('ask_ai', { 
                userInput, 
                activeTrackId: selectedTrackId ?? null, 
                playheadTime: globalState.playheadTime,
                chatHistory
            });

            // 2. Parse the AIExecutionTrace returned from Rust
            const trace: AIExecutionTrace = JSON.parse(rawResponse);
            console.log("🧠 Received AI Trace from Rust:", trace);

            // 🚨 Check for Schema/Governance Errors caught by Rust
            if (trace.errors && trace.errors.length > 0) {
                console.error("🛑 Rust AI Pipeline Errors:", trace.errors);
                return {
                    role: 'assistant',
                    content: "The audio engine rejected this command: " + trace.errors[0],
                    timestamp: Date.now(),
                    action: 'error'
                };
            }

            const commands = trace.execution_order || [];
            const displayMessage = trace.message || "Done.";

            // 3. Simple UI vs DSP Routing
            if (commands.length > 0) {
                const transportCommands = ['play', 'pause', 'record', 'rewind', 'seek', 'toggle_monitor', 'separate_stems', 'undo', 'redo', 'create_track', 'set_bpm'];
                
                const uiCommands = commands.filter((c: AICommand) => transportCommands.includes(c.action));
                const dspCommands = commands.filter((c: AICommand) => !transportCommands.includes(c.action));

                console.log(`🔀 Routing: ${uiCommands.length} UI Commands, ${dspCommands.length} DSP Commands.`);

                // Dispatch UI actions locally to Svelte
                uiCommands.forEach((cmd: AICommand) => {
                    window.dispatchEvent(new CustomEvent('ai-command', { detail: cmd }));
                });

                // Send DSP payload directly to Audio Engine
                if (dspCommands.length > 0) {
                    try {
                        await invoke('execute_ai_transaction', { 
                            version: "1.0",
                            commands: dspCommands // Rust already sorted and normalized these!
                        });
                        
                        setTimeout(() => {
                            window.dispatchEvent(new CustomEvent('refresh-project')); 
                        }, 150);
                    } catch (transactionError) {
                        console.error("🛑 DSP Execution Error:", transactionError);
                        return {
                            role: 'assistant',
                            content: `I tried to do that, but the Audio Engine prevented it: ${transactionError}`,
                            timestamp: Date.now(),
                            action: 'error'
                        };
                    }
                }
            }

            // Return the message from the LLM back to the chat UI
            return {
                role: 'assistant',
                content: displayMessage,
                timestamp: Date.now(),
                action: commands?.[0]?.action || 'none'
            };

        } catch (e) {
            console.error("Agent Error:", e);
            return {
                role: 'assistant',
                content: "System communication error. Check Rust logs.",
                timestamp: Date.now()
            };
        }
    }
}    

export const aiAgent = new AIAgent();