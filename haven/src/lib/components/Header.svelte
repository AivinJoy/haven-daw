<!-- haven\src\lib\components\Header.svelte -->

<script lang="ts">
  import { PenLine, Settings, User, Bell } from 'lucide-svelte';

  // Props
  let { projectName = $bindable("Untitled Project") } = $props();

  // Editing State
  let isEditing = $state(false);
  let inputRef = $state<HTMLInputElement>();

  function handleEdit() {
    isEditing = true;
    // Wait for DOM update to focus input
    setTimeout(() => inputRef?.focus(), 0);
  }

  function handleBlur() {
    isEditing = false;
    if (projectName.trim() === "") projectName = "Untitled Project";
  }

  function handleKey(e: KeyboardEvent) {
    if (e.key === 'Enter') handleBlur();
  }
</script>

<div class="h-14 w-full bg-[#1a1a2e] border-b border-white/10 flex items-center justify-between px-4 select-none z-50 relative shrink-0">
  
  <div class="flex items-center w-1/3 gap-3">
    <!-- <div class="w-8 h-8 rounded-lg bg-brand-blue flex items-center justify-center shadow-[0_0_15px_rgba(59,130,246,0.5)]">
        <span class="font-bold text-white text-lg font-sans">H</span>
    </div> -->
    <h1 class="text-lg font-light tracking-[0.2em] text-white/90 font-sans cursor-default hidden sm:block">
        HAVEN
    </h1>
  </div>

  <div class="flex-1 flex justify-center w-1/3">
    <div class="relative group flex items-center justify-center h-8">
        
        {#if isEditing}
            <input 
                bind:this={inputRef}
                type="text" 
                bind:value={projectName} 
                onblur={handleBlur}
                onkeydown={handleKey}
                class="bg-white/10 border border-brand-blue/50 rounded px-2 py-1 text-center text-sm font-semibold text-white focus:outline-none focus:ring-1 focus:ring-brand-blue w-64 shadow-inner"
            />
        {:else}
            <button 
                onclick={handleEdit}
                class="flex items-center gap-2 px-4 py-1 rounded-lg hover:bg-white/5 border border-transparent hover:border-white/10 transition-all text-white/90 group"
                title="Rename Project"
            >
                <span class="text-sm font-semibold tracking-wide">{projectName}</span>
                <PenLine size={12} class="opacity-0 group-hover:opacity-100 transition-opacity text-white/40 translate-y-1px" />
            </button>
        {/if}

    </div>
  </div>

  <div class="w-1/3 flex justify-end items-center gap-4">
    <button class="text-white/40 hover:text-white transition-colors"><Bell size={18} /></button>
    <button class="text-white/40 hover:text-white transition-colors"><Settings size={18} /></button>
    
    <div class="w-8 h-8 rounded-full bg-linear-to-tr from-purple-500 to-blue-500 border border-white/20 flex items-center justify-center shadow-lg cursor-pointer hover:scale-105 transition-transform">
        <User size={14} class="text-white" />
    </div>
  </div>

</div>