<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import { FolderOpen, Save, FileOutput, FilePlus } from 'lucide-svelte';

  const dispatch = createEventDispatcher();
</script>

<div class="absolute top-12 left-4 w-56 bg-[#1a1a2e] border border-white/10 rounded-lg shadow-xl z-100 flex flex-col py-1 overflow-hidden backdrop-blur-xl animate-in fade-in slide-in-from-top-2 duration-150">
    
    <button onclick={() => dispatch('new')} class="h-10 px-4 flex items-center gap-3 hover:bg-white/5 transition-colors text-white/80 hover:text-white text-sm text-left w-full">
        <FilePlus size={16} class="text-white/40" />
        New Project
    </button>

    <div class="h-px bg-white/5 my-1 mx-2"></div>

    <button onclick={() => dispatch('load')} class="h-10 px-4 flex items-center gap-3 hover:bg-white/5 transition-colors text-white/80 hover:text-white text-sm text-left w-full">
        <FolderOpen size={16} class="text-brand-blue" />
        Load Project
    </button>

    <button onclick={() => dispatch('save')} class="h-10 px-4 flex items-center gap-3 hover:bg-white/5 transition-colors text-white/80 hover:text-white text-sm text-left w-full">
        <Save size={16} class="text-emerald-400" />
        Save Project
    </button>

    <div class="h-px bg-white/5 my-1 mx-2"></div>

    <button onclick={() => dispatch('export')} class="h-10 px-4 flex items-center gap-3 hover:bg-white/5 transition-colors text-white/80 hover:text-white text-sm text-left w-full">
        <FileOutput size={16} class="text-purple-400" />
        Export to WAV
    </button>

</div>