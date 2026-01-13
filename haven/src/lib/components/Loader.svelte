<script lang="ts">
  import { Loader2 } from 'lucide-svelte';
  // Import the global store we created
  import { loader } from '$lib/stores/loader.svelte';
</script>

{#if loader.visible}
<div class="fixed inset-0 z-9999 flex items-center justify-center bg-[#0f0f16]/90 backdrop-blur-md transition-opacity duration-300">
    <div class="flex flex-col items-center gap-6 p-8 w-80">
        
        <div class="relative">
            <div class="absolute inset-0 bg-brand-blue/20 blur-xl rounded-full"></div>
            <Loader2 size={40} class="text-brand-blue animate-spin relative z-10" />
        </div>

        <div class="text-center space-y-1">
            <p class="text-lg font-light text-white animate-pulse">{loader.message}</p>
            <p class="text-xs text-white/40 font-mono">{Math.round(loader.progress)}%</p>
        </div>

        <div class="w-full h-1 bg-white/10 rounded-full overflow-hidden">
            <div 
                class="h-full bg-brand-blue transition-all duration-300 ease-out" 
                style="width: {loader.progress}%"
            ></div>
        </div>
    </div>
</div>
{/if}