<script lang="ts">
    import { Send, Bot, Loader2, Cpu } from 'lucide-svelte'; // <--- Added Loader2, Cpu
    import { aiAgent, type AIMessage } from '../managers/AIAgent';
    
    // --- NEW IMPORTS ---
    import { onMount } from 'svelte';
    import { listen } from '@tauri-apps/api/event';

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
    let reasoningProgress = $state(0);

    // --- NEW: Listen for Rust Events ---
    onMount(() => {
        const unlistenPromise = listen('progress-update', (event: any) => {
            const payload = event.payload; 
            
            if (payload.visible) {
                isReasoning = true;
                reasoningMessage = payload.message;
                reasoningProgress = payload.progress;
            } else {
                isReasoning = false;
                // Optional: Append a success message when done
                if (reasoningProgress >= 95) {
                    messages = [...messages, { 
                        role: 'assistant', 
                        content: "Task completed successfully.", 
                        timestamp: Date.now() 
                    }];
                }
            }
        });

        return () => {
            unlistenPromise.then(unlisten => unlisten());
        };
    });

    async function handleSubmit() {
        if (!input.trim() || isLoading || isReasoning) return; // Prevent double submit
        
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

    {#if messages.length > 0 || isReasoning}
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
                    <div class="flex w-full justify-start">
                        <div class="max-w-[80%] px-3 py-2 rounded-lg text-sm flex flex-col gap-2 bg-white/5 text-white/90 rounded-tl-none border border-brand-blue/30 shadow-[0_0_15px_rgba(59,130,246,0.15)]">
                            <div class="flex items-center gap-2 text-brand-blue text-[10px] font-bold uppercase tracking-wider">
                                <Cpu size={12} class="animate-spin-slow" />
                                AI Processing
                            </div>
                            <p class="text-xs text-gray-300">{reasoningMessage}</p>
                            <div class="w-full bg-black/40 h-1 rounded-full overflow-hidden">
                                <div class="bg-brand-blue h-full transition-all duration-300" style="width: {reasoningProgress}%"></div>
                            </div>
                        </div>
                    </div>
                {/if}
            </div>

            {#if !isHovered}
                <div class="absolute bottom-0 left-0 w-full h-12 flex items-center px-4 gap-3 bg-linear-to-t from-black/80 to-transparent">
                     
                     {#if isReasoning}
                        <div class="flex-1 flex flex-col justify-center h-full py-2 gap-1.5">
                             <div class="flex justify-between items-center text-xs">
                                <span class="text-brand-blue font-bold flex items-center gap-2">
                                    <Loader2 size={12} class="animate-spin" />
                                    {reasoningMessage}
                                </span>
                                <span class="text-white/50 font-mono text-[10px]">{Math.round(reasoningProgress)}%</span>
                             </div>
                             <div class="w-full bg-white/10 h-1 rounded-full overflow-hidden">
                                 <div 
                                    class="bg-brand-blue h-full shadow-[0_0_10px_rgba(59,130,246,0.8)] transition-all duration-300 ease-out" 
                                    style="width: {reasoningProgress}%"
                                 ></div>
                             </div>
                        </div>
                     {:else}
                        <Bot size={16} class="text-brand-blue animate-pulse" />
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
            placeholder={isReasoning ? "AI is working..." : "Ask Haven to split, mute, or pan..."}
            class="flex-1 bg-transparent border-none outline-none h-full px-4 text-sm text-white placeholder-white/30 disabled:opacity-50 disabled:cursor-wait"
            disabled={isLoading || isReasoning} 
        />

        <button 
            onclick={handleSubmit}
            disabled={!input.trim() || isLoading || isReasoning}
            class="h-9 w-9 rounded-full bg-brand-blue text-black flex items-center justify-center mr-1 hover:scale-105 active:scale-95 transition-all disabled:opacity-50 disabled:cursor-not-allowed"
            aria-label="Send Message"
        >
            {#if isLoading || isReasoning}
                <div class="w-4 h-4 border-2 border-black/30 border-t-black rounded-full animate-spin"></div>
            {:else}
                <Send size={16} class="ml-0.5" />
            {/if}
        </button>
    </div>

</div>

<style>
    .scrollbar-hide::-webkit-scrollbar { display: none; }
    .scrollbar-hide { -ms-overflow-style: none; scrollbar-width: none; }
    
    /* Optional slow spin for the CPU icon inside expanded view */
    @keyframes spin-slow {
        from { transform: rotate(0deg); }
        to { transform: rotate(360deg); }
    }
    .animate-spin-slow {
        animation: spin-slow 3s linear infinite;
    }
</style>