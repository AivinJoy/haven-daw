<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { X, Power } from "lucide-svelte";
  import Knob from './Knob.svelte';

  let { trackId, onClose } = $props();

  const KNOB_COLOR = "#f59e0b"; 

  let winX = $state(100);
  let winY = $state(100);
  let isDragging = false;
  let dragOffset = { x: 0, y: 0 };
  
  let loading = $state(true);

  let params = $state({
    is_active: true,
    threshold_db: -20.0,
    ratio: 4.0,
    attack_ms: 5.0,
    release_ms: 50.0,
    makeup_gain_db: 0.0
  });

  onMount(async () => {
    try {
      winX = (window.innerWidth / 2) - 225;
      winY = (window.innerHeight / 2) - 150;
      params = await invoke('get_compressor_state', { trackId });
    } catch (e) {
      console.error("Failed to fetch Compressor state from Rust:", e);
    } finally {
      loading = false;
    }
  });

  async function updateBackend() {
    try {
      await invoke('update_compressor', { trackId, params: $state.snapshot(params) });
    } catch (e) {
      console.error("Compressor update failed:", e);
    }
  }

  // Restrict the allowed keys to ONLY the numeric ones
  type NumericParams = "threshold_db" | "ratio" | "attack_ms" | "release_ms" | "makeup_gain_db";

  function handleParamChange(paramName: NumericParams, val: number) {
    (params as any)[paramName] = val; // Force the assignment
    updateBackend();
  }

  function toggleBypass() {
    params.is_active = !params.is_active;
    updateBackend();
  }

  // --- WINDOW DRAG LOGIC ---
  function startDrag(e: MouseEvent) {
    if ((e.target as HTMLElement).closest('button, select, input, .cursor-ns-resize')) return;
    isDragging = true;
    dragOffset = { x: e.clientX - winX, y: e.clientY - winY };
    window.addEventListener("mousemove", handleDrag);
    window.addEventListener("mouseup", stopDrag);
  }

  function handleDrag(e: MouseEvent) {
    if (!isDragging) return;
    winX = e.clientX - dragOffset.x;
    winY = e.clientY - dragOffset.y;
  }

  function stopDrag() {
    isDragging = false;
    window.removeEventListener("mousemove", handleDrag);
    window.removeEventListener("mouseup", stopDrag);
  }
</script>

<style>
  /* Global CSS to hide number input arrows cleanly across browsers */
  input[type=number]::-webkit-inner-spin-button, 
  input[type=number]::-webkit-outer-spin-button { 
    -webkit-appearance: none; 
    margin: 0; 
  }
  input[type=number] {
    -moz-appearance: textfield; 
    appearance: textfield; /* <-- ADDED THIS LINE */
  }
</style>

<div 
  class="fixed flex flex-col w-[450px] overflow-hidden bg-[#1a1a1d] border border-white/10 rounded-xl select-none text-zinc-400 font-sans"
  style={`left: ${winX}px; top: ${winY}px; z-index: 9999; box-shadow: 0 40px 80px rgba(0,0,0,0.8);`}
>
  <div 
    class="h-12 bg-[#202023] border-b border-black/30 flex items-center justify-between px-4 cursor-move"
    onmousedown={startDrag} role="button" tabindex="0"
  >
    <div class="flex items-center gap-3 w-24">
      <button 
        onclick={toggleBypass}
        class={`flex items-center gap-2 px-3 py-1 rounded text-xs uppercase tracking-wider font-bold transition-colors border border-white/5 shadow-sm ${!params.is_active ? 'bg-red-500/20 text-red-400 border-red-500/50' : 'bg-[#1a1a1d] hover:bg-[#3f3f46] text-zinc-500'}`}
      >
          <Power size={12} class={!params.is_active ? "text-red-400" : "text-zinc-500"} />
          <span>Bypass</span>
      </button>
    </div>

    <h2 class="text-zinc-400 text-sm font-semibold tracking-widest uppercase text-center flex-1">Compressor</h2>
    
    <div class="flex items-center gap-3 w-24 justify-end">
      <button onclick={onClose} class="text-zinc-500 hover:text-white transition-colors">
          <X size={18} />
      </button>
    </div>
  </div>

  <div class="p-6 bg-[#131315] relative">
    <div class="absolute inset-0 bg-[url('/noise.png')] opacity-[0.03] pointer-events-none"></div>

    {#if loading}
       <div class="h-[200px] flex items-center justify-center text-zinc-600 font-bold tracking-widest uppercase text-sm">
           Initializing...
       </div>
    {:else}
      <div class="relative z-10 flex flex-col gap-6" style={`opacity: ${params.is_active ? '1.0' : '0.4'}; transition: opacity 0.2s;`}>
        
        <div class="flex justify-between items-end gap-2">
          
          <div class="flex flex-col items-center gap-3">
            <span class="text-[10px] text-zinc-500 font-bold uppercase tracking-widest">Attack</span>
            <Knob 
              value={params.attack_ms} 
              min={1} max={200} step={1} color={KNOB_COLOR} defaultValue={5.0}
              onChange={(val) => handleParamChange('attack_ms', val)} 
            />
            <div class="bg-[#0f0f11] border border-white/5 rounded px-2 py-1 shadow-[inset_0_2px_4px_rgba(0,0,0,0.5)] flex items-center justify-center">
              <input type="number" bind:value={params.attack_ms} onchange={updateBackend} class="bg-transparent text-xs text-zinc-300 font-mono w-8 text-right focus:outline-none focus:text-white" />
              <span class="text-xs text-zinc-500 font-mono ml-1">ms</span>
            </div>
          </div>
          
          <div class="flex flex-col items-center gap-3">
            <span class="text-[10px] text-zinc-500 font-bold uppercase tracking-widest">Release</span>
            <Knob 
              value={params.release_ms} 
              min={10} max={1000} step={1} color={KNOB_COLOR} defaultValue={50.0}
              onChange={(val) => handleParamChange('release_ms', val)} 
            />
            <div class="bg-[#0f0f11] border border-white/5 rounded px-2 py-1 shadow-[inset_0_2px_4px_rgba(0,0,0,0.5)] flex items-center justify-center">
              <input type="number" bind:value={params.release_ms} onchange={updateBackend} class="bg-transparent text-xs text-zinc-300 font-mono w-8 text-right focus:outline-none focus:text-white" />
              <span class="text-xs text-zinc-500 font-mono ml-1">ms</span>
            </div>
          </div>
          
          <div class="flex flex-col items-center gap-3">
            <span class="text-[10px] text-zinc-500 font-bold uppercase tracking-widest">Threshold</span>
            <Knob 
              value={params.threshold_db} 
              min={-60} max={0} step={0.1} color={KNOB_COLOR} defaultValue={-20.0}
              onChange={(val) => handleParamChange('threshold_db', val)} 
            />
            <div class="bg-[#0f0f11] border border-white/5 rounded px-2 py-1 shadow-[inset_0_2px_4px_rgba(0,0,0,0.5)] flex items-center justify-center">
              <input type="number" step="0.1" bind:value={params.threshold_db} onchange={updateBackend} class="bg-transparent text-xs text-zinc-300 font-mono w-10 text-right focus:outline-none focus:text-white" />
              <span class="text-xs text-zinc-500 font-mono ml-1">dB</span>
            </div>
          </div>

        </div>

        <div class="flex justify-evenly items-end gap-4 mt-2">
          
          <div class="flex flex-col items-center gap-3">
            <span class="text-[10px] text-zinc-500 font-bold uppercase tracking-widest">Ratio</span>
            <Knob 
              value={params.ratio} 
              min={1} max={20} step={0.1} color={KNOB_COLOR} defaultValue={4.0}
              onChange={(val) => handleParamChange('ratio', val)} 
            />
            <div class="bg-[#0f0f11] border border-white/5 rounded px-2 py-1 shadow-[inset_0_2px_4px_rgba(0,0,0,0.5)] flex items-center justify-center">
              <input type="number" step="0.1" bind:value={params.ratio} onchange={updateBackend} class="bg-transparent text-xs text-zinc-300 font-mono w-8 text-right focus:outline-none focus:text-white" />
              <span class="text-xs text-zinc-500 font-mono ml-1">:1</span>
            </div>
          </div>
          
          <div class="flex flex-col items-center gap-3">
            <span class="text-[10px] text-zinc-500 font-bold uppercase tracking-widest">Makeup</span>
            <Knob 
              value={params.makeup_gain_db} 
              min={0} max={24} step={0.1} color={KNOB_COLOR} defaultValue={0.0}
              onChange={(val) => handleParamChange('makeup_gain_db', val)} 
            />
            <div class="bg-[#0f0f11] border border-white/5 rounded px-2 py-1 shadow-[inset_0_2px_4px_rgba(0,0,0,0.5)] flex items-center justify-center">
              <span class="text-xs text-zinc-500 font-mono mr-0.5">+</span>
              <input type="number" step="0.1" bind:value={params.makeup_gain_db} onchange={updateBackend} class="bg-transparent text-xs text-zinc-300 font-mono w-8 text-right focus:outline-none focus:text-white" />
              <span class="text-xs text-zinc-500 font-mono ml-1">dB</span>
            </div>
          </div>

        </div>
      </div>
    {/if}
  </div>
</div>