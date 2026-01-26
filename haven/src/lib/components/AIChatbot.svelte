<script lang="ts">
    import { Send, Bot } from 'lucide-svelte';
    import { aiAgent, type AIMessage } from '../managers/AIAgent';
    
    // Props to get context
    let { tracks = [] } = $props();

    let input = $state("");
    let isLoading = $state(false);
    let messages = $state<AIMessage[]>([]);
    
    // UI State for hover expansion
    let isHovered = $state(false);

    async function handleSubmit() {
        if (!input.trim() || isLoading) return;
        
        const userMsg: AIMessage = { 
            role: 'user', 
            content: input, 
            timestamp: Date.now() 
        };

        // Keep a reference to the history BEFORE adding the new message
        // (Or include it? Usually better to send history + current input separately, 
        // but our backend logic appends current input. So let's pass 'messages' as is).
        const historyToSend = [...messages];
        
        // Add user message immediately (right aligned)
        messages = [...messages, userMsg];
        const currentInput = input;
        input = "";
        isLoading = true;

        // PASS HISTORY HERE
        const response = await aiAgent.sendMessage(currentInput, tracks, historyToSend);
        
        messages = [...messages, response];
        isLoading = false;
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

    {#if messages.length > 0}
        <div 
            class="w-full pointer-events-auto transition-all duration-300 ease-in-out overflow-hidden rounded-2xl
                   bg-black/60 backdrop-blur-xl border border-white/10 shadow-2xl flex flex-col-reverse"
            style:height={isHovered ? '400px' : '48px'} 
            onmouseenter={() => isHovered = true}
            onmouseleave={() => isHovered = false}
            role="region"
            aria-label="AI Chat Response History"
        >
            <div class="flex-1 overflow-y-auto p-4 flex flex-col gap-3 scrollbar-hide">
                {#each messages as msg}
                    <div class={`flex w-full ${msg.role === 'user' ? 'justify-end' : 'justify-start'}`}>
                        
                        <div class={`max-w-[80%] px-3 py-2 rounded-lg text-sm flex items-center gap-2
                            ${msg.role === 'user' 
                                ? 'bg-brand-blue/20 text-brand-blue rounded-tr-none border border-brand-blue/10' 
                                : 'bg-white/5 text-white/90 rounded-tl-none border border-white/5'
                            }`}
                        >
                            {#if msg.role === 'assistant'} <Bot size={14} class="opacity-50" /> {/if}
                            <span>{msg.content}</span>
                        </div>

                    </div>
                {/each}
            </div>

            {#if !isHovered}
                <div class="absolute bottom-0 left-0 w-full h-12 flex items-center px-4 gap-3 bg-linear-to-t from-black/80 to-transparent">
                     <Bot size={16} class="text-brand-blue animate-pulse" />
                     <span class="text-sm text-white/90 truncate">
                        {messages[messages.length - 1].content}
                     </span>
                     <span class="ml-auto text-[10px] text-white/40 uppercase tracking-widest">
                        Hover History
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
            placeholder="Ask Haven to split, mute, or pan..."
            class="flex-1 bg-transparent border-none outline-none h-full px-4 text-sm text-white placeholder-white/30"
            disabled={isLoading}
        />

        <button 
            onclick={handleSubmit}
            disabled={!input.trim() || isLoading}
            class="h-9 w-9 rounded-full bg-brand-blue text-black flex items-center justify-center mr-1 hover:scale-105 active:scale-95 transition-all disabled:opacity-50 disabled:cursor-not-allowed"
            aria-label="Send Message"
        >
            {#if isLoading}
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
</style>