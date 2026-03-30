<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { X, Power } from "lucide-svelte";
  import Knob from './Knob.svelte';

  let { trackId, onClose } = $props();
  const KNOB_COLOR = "#0ea5e9"; // Sky Blue to differentiate from Compressor

  let winX = $state(100);
  let winY = $state(100);
  let isDragging = false;
  let dragOffset = { x: 0, y: 0 };
  
  let loading = $state(true);
  let params = $state({
    is_active: true,
    room_size: 0.8,
    damping: 0.5,
    pre_delay_ms: 10.0,
    width: 1.0,
    low_cut_hz: 100.0,
    high_cut_hz: 8000.0,
    mix: 0.3
  });

  onMount(async () => {
    try {
      winX = (window.innerWidth / 2) - 260; // Slightly wider window
      winY = (window.innerHeight / 2) - 150;
      
      const state: any = await invoke('get_reverb_state', { trackId });
      params = { ...params, ...state };
    } catch (e) {
      console.error("Failed to fetch Reverb state from Rust:", e);
    } finally {
      loading = false;
    }
  });

  type ReverbParams = "room_size" | "damping" | "pre_delay_ms" | "width" | "low_cut_hz" | "high_cut_hz" | "mix" | "is_active";

  async function updateBackendParam(paramName: ReverbParams, val: number | boolean) {
    (params as any)[paramName] = val;
    
    // Map frontend variable names to the string keys expected by our generic Rust `set_param` match arm
    let rustParamName = paramName as string;
    if (paramName === 'pre_delay_ms') rustParamName = 'pre_delay';
    if (paramName === 'low_cut_hz') rustParamName = 'low_cut';
    if (paramName === 'high_cut_hz') rustParamName = 'high_cut';
    if (paramName === 'is_active') rustParamName = 'active';

    try {
      await invoke('set_effect_param', { 
        trackId, 
        effect: 'reverb', 
        param: rustParamName, 
        value: typeof val === 'boolean' ? (val ? 1.0 : 0.0) : val 
      });
    } catch (e) {
      console.error(`Reverb update failed for ${paramName}:`, e);
    }
  }

  function toggleBypass() {
    updateBackendParam('is_active', !params.is_active);
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
  input[type=number]::-webkit-inner-spin-button, 
  input[type=number]::-webkit-outer-spin-button { 
    -webkit-appearance: none;
    margin: 0; 
  }
  input[type=number] {
    -moz-appearance: textfield; 
    appearance: textfield;
  }
</style>

<div 
  class="fixed flex flex-col w-[520px] overflow-hidden bg-[#1a1a1d] border border-white/10 rounded-xl select-none text-zinc-400 font-sans"
  style={`left: ${winX}px; top: ${winY}px; z-index: 9999; box-shadow: 0 40px 80px rgba(0,0,0,0.8);`}
>
  <div 
    class="h-12 bg-[#202023] border-b border-black/30 flex items-center justify-between px-4 cursor-move"
    onmousedown={startDrag} role="button" tabindex="0"
  >
    <div class="flex items-center gap-3 w-24">
      <button 
        onclick={toggleBypass}
        class={`flex items-center gap-2 px-3 py-1 rounded text-xs uppercase tracking-wider font-bold transition-colors border border-white/5 shadow-sm ${!params.is_active ?
        'bg-red-500/20 text-red-400 border-red-500/50' : 'bg-[#1a1a1d] hover:bg-[#3f3f46] text-zinc-500'}`}
      >
          <Power size={12} class={!params.is_active ? "text-red-400" : "text-zinc-500"} />
          <span>Bypass</span>
      </button>
    </div>

    <h2 class="text-zinc-400 text-sm font-semibold tracking-widest uppercase text-center flex-1">Reverb</h2>
    
    <div class="flex items-center gap-3 w-24 justify-end">
      <button onclick={onClose} class="text-zinc-500 hover:text-white transition-colors">
          <X size={18} />
      </button>
    </div>
  </div>

  <div class="p-6 bg-[#131315] relative">
    <div class="absolute inset-0 bg-[url('/noise.png')] opacity-[0.03] pointer-events-none"></div>

    {#if loading}
       <div class="h-60 flex items-center justify-center text-zinc-600 font-bold tracking-widest uppercase text-sm">
           Initializing...
       </div>
    {:else}
      <div class="relative z-10 flex flex-col gap-8" style={`opacity: ${params.is_active ? '1.0' : '0.4'}; transition: opacity 0.2s;`}>
        
        <div class="flex justify-between items-end gap-2">
          
          <div class="flex flex-col items-center gap-3">
            <span class="text-[10px] text-zinc-500 font-bold uppercase tracking-widest">Pre-Delay</span>
            <Knob 
              value={params.pre_delay_ms} 
              min={0} max={250} step={1} color={KNOB_COLOR} defaultValue={10.0}
              onChange={(val) => updateBackendParam('pre_delay_ms', val)} 
            />
            <div class="bg-[#0f0f11] border border-white/5 rounded px-2 py-1 shadow-[inset_0_2px_4px_rgba(0,0,0,0.5)] flex items-center justify-center">
              <input type="number" bind:value={params.pre_delay_ms} onchange={(e) => updateBackendParam('pre_delay_ms', parseFloat((e.target as HTMLInputElement).value))} class="bg-transparent text-xs text-zinc-300 font-mono w-8 text-right focus:outline-none focus:text-white" />
              <span class="text-xs text-zinc-500 font-mono ml-1">ms</span>
            </div>
          </div>

          <div class="flex flex-col items-center gap-3">
            <span class="text-[10px] text-zinc-500 font-bold uppercase tracking-widest">Size</span>
            <Knob 
              value={params.room_size} 
              min={0} max={1} step={0.01} color={KNOB_COLOR} defaultValue={0.8}
              onChange={(val) => updateBackendParam('room_size', val)} 
            />
            <div class="bg-[#0f0f11] border border-white/5 rounded px-2 py-1 shadow-[inset_0_2px_4px_rgba(0,0,0,0.5)] flex items-center justify-center">
              <input type="number" step="0.01" bind:value={params.room_size} onchange={(e) => updateBackendParam('room_size', parseFloat((e.target as HTMLInputElement).value))} class="bg-transparent text-xs text-zinc-300 font-mono w-10 text-right focus:outline-none focus:text-white" />
            </div>
          </div>
          
          <div class="flex flex-col items-center gap-3">
            <span class="text-[10px] text-zinc-500 font-bold uppercase tracking-widest">Damping</span>
            <Knob 
              value={params.damping} 
              min={0} max={1} step={0.01} color={KNOB_COLOR} defaultValue={0.5}
              onChange={(val) => updateBackendParam('damping', val)} 
            />
            <div class="bg-[#0f0f11] border border-white/5 rounded px-2 py-1 shadow-[inset_0_2px_4px_rgba(0,0,0,0.5)] flex items-center justify-center">
              <input type="number" step="0.01" bind:value={params.damping} onchange={(e) => updateBackendParam('damping', parseFloat((e.target as HTMLInputElement).value))} class="bg-transparent text-xs text-zinc-300 font-mono w-10 text-right focus:outline-none focus:text-white" />
            </div>
          </div>

          <div class="flex flex-col items-center gap-3">
            <span class="text-[10px] text-zinc-500 font-bold uppercase tracking-widest">Width</span>
            <Knob 
              value={params.width} 
              min={0} max={1} step={0.01} color={KNOB_COLOR} defaultValue={1.0}
              onChange={(val) => updateBackendParam('width', val)} 
            />
            <div class="bg-[#0f0f11] border border-white/5 rounded px-2 py-1 shadow-[inset_0_2px_4px_rgba(0,0,0,0.5)] flex items-center justify-center">
              <input type="number" step="0.01" bind:value={params.width} onchange={(e) => updateBackendParam('width', parseFloat((e.target as HTMLInputElement).value))} class="bg-transparent text-xs text-zinc-300 font-mono w-10 text-right focus:outline-none focus:text-white" />
            </div>
          </div>

        </div>

        <div class="flex justify-evenly items-end gap-4 mt-2">
          
          <div class="flex flex-col items-center gap-3">
            <span class="text-[10px] text-zinc-500 font-bold uppercase tracking-widest">Low Cut</span>
            <Knob 
              value={params.low_cut_hz} 
              min={20} max={1000} step={1} color={KNOB_COLOR} mapMode="log" defaultValue={100.0}
              onChange={(val) => updateBackendParam('low_cut_hz', val)} 
            />
            <div class="bg-[#0f0f11] border border-white/5 rounded px-2 py-1 shadow-[inset_0_2px_4px_rgba(0,0,0,0.5)] flex items-center justify-center">
              <input type="number" step="1" bind:value={params.low_cut_hz} onchange={(e) => updateBackendParam('low_cut_hz', parseFloat((e.target as HTMLInputElement).value))} class="bg-transparent text-xs text-zinc-300 font-mono w-10 text-right focus:outline-none focus:text-white" />
              <span class="text-xs text-zinc-500 font-mono ml-1">Hz</span>
            </div>
          </div>
          
          <div class="flex flex-col items-center gap-3">
            <span class="text-[10px] text-zinc-500 font-bold uppercase tracking-widest">High Cut</span>
            <Knob 
              value={params.high_cut_hz} 
              min={1000} max={20000} step={100} color={KNOB_COLOR} mapMode="log" defaultValue={8000.0}
              onChange={(val) => updateBackendParam('high_cut_hz', val)} 
            />
            <div class="bg-[#0f0f11] border border-white/5 rounded px-2 py-1 shadow-[inset_0_2px_4px_rgba(0,0,0,0.5)] flex items-center justify-center">
              <input type="number" step="100" bind:value={params.high_cut_hz} onchange={(e) => updateBackendParam('high_cut_hz', parseFloat((e.target as HTMLInputElement).value))} class="bg-transparent text-xs text-zinc-300 font-mono w-12 text-right focus:outline-none focus:text-white" />
              <span class="text-xs text-zinc-500 font-mono ml-1">Hz</span>
            </div>
          </div>

          <div class="flex flex-col items-center gap-3">
            <span class="text-[10px] text-zinc-500 font-bold uppercase tracking-widest">Mix</span>
            <Knob 
              value={params.mix} 
              min={0} max={1} step={0.01} color={KNOB_COLOR} defaultValue={0.3}
              onChange={(val) => updateBackendParam('mix', val)} 
            />
            <div class="bg-[#0f0f11] border border-white/5 rounded px-2 py-1 shadow-[inset_0_2px_4px_rgba(0,0,0,0.5)] flex items-center justify-center">
              <input type="number" step="0.01" bind:value={params.mix} onchange={(e) => updateBackendParam('mix', parseFloat((e.target as HTMLInputElement).value))} class="bg-transparent text-xs text-zinc-300 font-mono w-10 text-right focus:outline-none focus:text-white" />
            </div>
          </div>

        </div>
      </div>
    {/if}
  </div>
</div>