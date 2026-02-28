<script lang="ts">
    import { Send, Bot, Loader2, Cpu, Square, X, Check } from 'lucide-svelte'; // <--- Added Loader2, Cpu
    import { aiAgent, type AIMessage } from '../managers/AIAgent';
    
    // --- NEW IMPORTS ---
    import { onMount } from 'svelte';
    import { listen } from '@tauri-apps/api/event';
	import { invoke } from '@tauri-apps/api/core';

    // Props to get context
    let { tracks = [] } = $props();

    let input = $state("");
    let isLoading = $state(false);
    let messages = $state<AIMessage[]>([]);
    
    // UI State for hover expansion
    let isHovered = $state(false);

    // --- NEW: AI Reasoning State ---
    let isReasoning = $state(false);
    let reasoningMessage = $state("");

    let currentJobId = $state<string | null>(null);
    let awaitingConfirmation = $state(false);

    // --- NEW: Listen for Rust Events ---
    onMount(() => {
    // 1. Job Started (Capture ID for Cancel)
        const unlistenStart = listen('ai-job-started', (event: any) => {
            currentJobId = event.payload;
            isReasoning = true;
            awaitingConfirmation = false;
        });

        // 2. Progress (Update UI Bubble)
        const unlistenProgress = listen('ai-progress', (event: any) => {
            const payload = event.payload;
            if (payload.visible) {
                isReasoning = true;
                reasoningMessage = payload.message;
            } else {
                isReasoning = false;
            }
        });

        // 3. Job Complete (Ask for Confirmation)
        const unlistenComplete = listen('ai-job-complete', (event: any) => {
            isReasoning = false;
            awaitingConfirmation = true;
         
            currentJobId = event.payload;

            messages = [...messages, { 
                role: 'assistant', 
                content: "Separation complete. Type 'Replace', 'Mute', or 'Keep' to import, or 'No' to discard.", 
                timestamp: Date.now() 
            }];
        });

        return () => {
            unlistenStart.then(u => u());
            unlistenProgress.then(u => u());
            unlistenComplete.then(u => u());
        };
    });

    async function handleStop() {
        if (currentJobId) {
            reasoningMessage = "Cancelling...";
            await invoke('cancel_ai_job', {jobId: currentJobId});
            isReasoning = false;
            currentJobId = null;
        }
    }

    async function handleConfirm(action: string) {
        if (!currentJobId) return;

        // Optimistic UI Update: Instant feedback
        awaitingConfirmation = false;

        if (action !== 'discard') {
            isReasoning = true;
            reasoningMessage = "importing Stems..."
            await invoke('commit_pending_stems', { jobId: currentJobId, importAction: action });
            messages = [...messages, { role: 'assistant', content: `Imported stems (${action} original).`, timestamp: Date.now() }];
        } else {
            await invoke('discard_pending_stems', { jobId: currentJobId });
            messages = [...messages, { role: 'assistant', content: "Discarded.", timestamp: Date.now() }];
        }

        isReasoning = false;
        currentJobId = null;
        window.dispatchEvent(new CustomEvent('refresh-project'));
    }

    async function handleSubmit() {
        if (!input.trim() || isLoading || isReasoning) return; // Prevent double submit
        
        if (awaitingConfirmation) {
            const text = input.trim().toLowerCase();

            const originalInput = input;
            input = "";

            messages = [...messages, { role: 'user', content: originalInput, timestamp: Date.now()} ];

            if (['replace', 'r'].includes(text)) {
                await handleConfirm('replace');
            } else if (['mute', 'm'].includes(text)) {
                await handleConfirm('mute');
            } else if (['keep', 'k'].includes(text)) {
                await handleConfirm('keep');
            } else if (['no', 'n', 'cancel', 'discard', 'stop'].includes(text)) {
                await handleConfirm('discard');
            } else {
                // If they type gibberish, ask again
                messages = [...messages, { role: 'assistant', content: "Please type 'Replace', 'Mute', 'Keep', or 'No'.", timestamp: Date.now() }];
            }
            return;
        }

        const userMsg: AIMessage = { 
            role: 'user', 
            content: input, 
            timestamp: Date.now() 
        };

        const historyToSend = [...messages];
        
        messages = [...messages, userMsg];
        const currentInput = input;
        input = "";
        isLoading = true;

        try {
            const response = await aiAgent.sendMessage(currentInput, tracks, historyToSend);
            messages = [...messages, response];
        } catch (e) {
            messages = [...messages, { role: 'assistant', content: "Error connecting to AI.", timestamp: Date.now() }];
        } finally {
            isLoading = false;
        }
    }

    function handleKeydown(e: KeyboardEvent) {
        if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            handleSubmit();
        }
    }
</script>

<div 
    class="fixed bottom-6 left-1/2 -translate-x-1/2 z-50 flex flex-col items-center gap-3 w-[600px] pointer-events-none"
>

    {#if messages.length > 0 || isReasoning || awaitingConfirmation}
        <div 
            class="w-full pointer-events-auto transition-all duration-300 ease-in-out overflow-hidden rounded-2xl
                   bg-black/60 backdrop-blur-xl border border-white/10 shadow-2xl flex flex-col-reverse"
            style:height={isHovered ? '400px' : '48px'} 
            onmouseenter={() => isHovered = true}
            onmouseleave={() => isHovered = false}
            role="region"
            aria-label="AI Chat Response History"
        >
            <div class={`flex-1 overflow-y-auto p-4 flex flex-col gap-3 scrollbar-hide transition-opacity duration-300 ${isHovered ? 'opacity-100': 'opacity-0 pointer-events-none'}`}>
                {#each messages as msg}
                    <div class={`flex w-full ${msg.role === 'user' ? 'justify-end' : 'justify-start'}`}>
                        
                        <div class={`max-w-[80%] px-3 py-2 rounded-lg text-sm flex items-center gap-2
                            ${msg.role === 'user' 
                                ? 'bg-brand-blue/20 text-brand-blue rounded-tr-none border border-brand-blue/10' 
                                : 'bg-white/5 text-white/90 rounded-tl-none border border-white/5'
                            }`}>
                            {#if msg.role === 'assistant'} <Bot size={14} class="opacity-50" /> {/if}
                            <span>{msg.content}</span>
                        </div>

                    </div>
                {/each}

                {#if isReasoning}
                    <div class="flex w-full justify-start animate-in fade-in slide-in-from-bottom-2">
                        <div class="max-w-[80%] px-3 py-2 rounded-lg text-sm flex items-center gap-2 bg-white/5 text-brand-blue/80 rounded-tl-none border border-brand-blue/20">
                            <Loader2 size={12} class="animate-spin shrink-0" />
                            <span class="text-xs font-mono">{reasoningMessage}</span>
                        </div>
                    </div>
                {/if}
            </div>    
            {#if !isHovered}
                <div class="absolute bottom-0 left-0 w-full h-12 flex items-center px-4 gap-3 bg-linear-to-t from-black/80 to-transparent">
                     {#if isReasoning}
                         <div class="flex-1 flex items-center h-full gap-2">
                             <Loader2 size={14} class="text-brand-blue animate-spin" />
                             <span class="text-xs text-brand-blue font-mono">
                                {reasoningMessage}
                             </span>
                        </div>
                      {:else}
                        <Bot size={16} class="text-brand-blue" />
                        <span class="text-sm text-white/90 truncate flex-1">
                            {[...messages].reverse().find(m => m.role === 'assistant')?.content || ''}
                        </span>
                     {/if}
                     
                     <span class="ml-auto text-[10px] text-white/40 uppercase tracking-widest pl-2">
                         {isReasoning ? 'Processing' : 'Hover'}
                     </span>
                </div>
            {/if}
        </div>
    {/if}    

    <div class="pointer-events-auto w-full h-12 rounded-full bg-white/5 backdrop-blur-xl border border-white/10 shadow-lg flex items-center px-1 relative transition-all focus-within:bg-white/10 focus-within:border-white/20">
        
        <input 
            type="text" 
            bind:value={input}
            onkeydown={handleKeydown}
            placeholder={isReasoning ? "Working..." : awaitingConfirmation ? "Type 'Replace', 'Mute', 'Keep' or 'No'... " : "Ask Haven to split, mute, or pan..."}
            class="flex-1 bg-transparent border-none outline-none h-full px-4 text-sm text-white placeholder-white/30 disabled:opacity-50 disabled:cursor-wait"
            disabled={isLoading || isReasoning} 
        />

        {#if isReasoning}
            <button 
                onclick={handleStop}
                class="h-9 w-9 rounded-full bg-red-500/20 text-red-500 border border-red-500/50 flex items-center justify-center mr-1 hover:bg-red-500 hover:text-white transition-all active:scale-95"
                title="Cancel Operation"
            >
                <Square size={12} fill="currentColor" />
            </button>
        {:else}
            <button 
                onclick={handleSubmit}
                disabled={!input.trim() || isLoading}
                class="h-9 w-9 rounded-full bg-brand-blue text-black flex items-center justify-center mr-1 hover:scale-105 active:scale-95 transition-all disabled:opacity-50 disabled:cursor-not-allowed"
                aria-label="Send Message"
            >
                {#if isLoading}
                    <Loader2 size={12} class="animate-spin" />
                {:else}
                    <Send size={12} class="ml-0.5" />
                {/if}
            </button>
        {/if}
    </div>

</div>

<style>
    .scrollbar-hide::-webkit-scrollbar { display: none; }
    .scrollbar-hide { -ms-overflow-style: none; scrollbar-width: none; }
</style>