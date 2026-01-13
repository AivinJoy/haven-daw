<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { X, Power, SlidersHorizontal, ChevronDown } from "lucide-svelte";
  import { onMount } from "svelte";
  import Knob from "./Knob.svelte";

  let { trackIndex, onClose } = $props();

  let loading = $state(true);
  let bands = $state<any[]>([]); 
  
  let winX = $state(100);
  let winY = $state(100);
  let isDragging = false;
  let dragOffset = { x: 0, y: 0 };

  // --- CONFIG (Using Explicit Hex Colors) ---
  const bandConfig = [
    { label: 'Low',      color: '#fbbf24' }, // Amber-400
    { label: 'Mid',      color: '#22d3ee' }, // Cyan-400
    { label: 'High Mid', color: '#a855f7' }, // Purple-500
    { label: 'High',     color: '#ec4899' }, // Pink-500
  ];

  const filterTypes = [
    "LowPass", "HighPass", "Peaking", "LowShelf", "HighShelf", "Notch", "BandPass"
  ];

  onMount(async () => {
    try {
      bands = await invoke("get_eq_state", { trackIndex });
      loading = false;
    } catch (e) {
      console.error("Failed to load EQ:", e);
    }
  });

  async function updateBand(bandIndex: number, param: string, value: any) {
    bands[bandIndex][param] = value; // Optimistic Update

    const band = bands[bandIndex];
    const args = {
      track_index: trackIndex,
      band_index: bandIndex,
      filter_type: band.filter_type,
      freq: parseFloat(band.freq),
      q: parseFloat(band.q),
      gain: parseFloat(band.gain),
      active: !!band.active,
    };

    try {
      await invoke("update_eq", { args });
    } catch (e) {
      console.error("EQ Update Failed:", e);
    }
  }

  // --- WINDOW DRAG ---
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

  function formatFreqLabel(hz: number) {
    if (hz >= 1000) return `${(hz/1000).toFixed(1)} kHz`;
    return `${Math.round(hz)} Hz`;
  }
</script>

<div 
  class="fixed z-50 w-[860px] bg-[#1a1a1d] rounded-xl shadow-2xl flex flex-col overflow-hidden border border-white/10 select-none text-zinc-400 font-sans"
  style={`left: ${winX}px; top: ${winY}px; box-shadow: 0 40px 80px rgba(0,0,0,0.8);`}
>
  <div 
    class="h-12 bg-[#202023] border-b border-black/30 flex items-center justify-between px-4 cursor-move"
    onmousedown={startDrag} role="button" tabindex="0"
  >
    <button class="flex items-center gap-2 bg-eq-panel hover:bg-[#3f3f46] px-3 py-1 rounded text-xs uppercase tracking-wider font-bold transition-colors border border-white/5 shadow-sm">
        <Power size={12} class="text-zinc-500" />
        <span>Bypass</span>
    </button>

    <h2 class="text-zinc-400 text-sm font-semibold tracking-widest uppercase">Biquad Equalizer</h2>

    <div class="flex items-center gap-3">
        <button class="flex items-center gap-2 bg-eq-panel hover:bg-[#3f3f46] px-3 py-1 rounded text-xs transition-colors border border-white/5 shadow-sm">
            <span>Presets</span>
            <SlidersHorizontal size={12} class="text-zinc-500" />
        </button>
        <button onclick={onClose} class="text-zinc-500 hover:text-white transition-colors">
             <X size={18} />
        </button>
    </div>
  </div>

  <div class="p-8 bg-[#131315] relative">
    <div class="absolute inset-0 bg-[url('/noise.png')] opacity-[0.03] pointer-events-none"></div>

    {#if loading}
      <div class="h-[400px] flex items-center justify-center text-zinc-600">Initializing...</div>
    {:else}
    <div class="grid grid-cols-4 gap-6 relative z-10">
      {#each bands as band, i}
        {@const config = bandConfig[i]}
        
        <div class="bg-[#1a1a1d] rounded-lg p-5 flex flex-col items-center border border-white/5 shadow-[0_4px_10px_rgba(0,0,0,0.3)] relative group">
            
            <div class="w-full flex flex-col items-center mb-4">
                <h3 
                    class="text-base font-bold drop-shadow-md mb-2" 
                    style={`color: ${config.color}`}
                >
                    {config.label}
                </h3>
                
                <div class="relative w-full group/select">
                    <select 
                        value={band.filter_type}
                        onchange={(e) => updateBand(i, 'filter_type', e.currentTarget.value)}
                        class="w-full bg-transparent text-[10px] uppercase font-bold text-zinc-600 py-1 text-center appearance-none focus:outline-none hover:text-zinc-400 cursor-pointer"
                    >
                        {#each filterTypes as type}
                            <option value={type} class="bg-eq-panel text-zinc-400">{type}</option>
                        {/each}
                    </select>
                </div>
            </div>

            <div class="mb-5 relative">
                <Knob 
                    bind:value={band.freq} 
                    min={20} max={20000} step={1} 
                    size="lg"
                    mapMode="log"
                    color={config.color}
                    onChange={(val) => updateBand(i, 'freq', val)}
                />
            </div>

            <div class="w-full flex items-center justify-between mb-2 px-1">
                <span class="text-[10px] font-bold uppercase text-zinc-500">Freq</span>
                
                <button 
                    type="button"
                    onclick={(e) => {
                        e.stopPropagation();
                        updateBand(i, 'active', !band.active);
                    }}
                    class="w-8 h-4 rounded-full p-0.5 transition-colors border border-white/5 relative cursor-pointer"
                    style={`background-color: ${band.active ? config.color + '33' : '#0f0f11'}`} 
                    aria-label="Toggle Band"
                >
                    <div 
                        class="w-3 h-3 rounded-full shadow-sm transition-transform absolute top-0.5 left-0.5"
                        style={`
                            transform: translateX(${band.active ? '16px' : '0'});
                            background-color: ${band.active ? config.color : '#3f3f46'};
                            box-shadow: ${band.active ? `0 0 6px ${config.color}` : 'none'};
                        `}
                    ></div>
                </button>
            </div>

            <div class="w-full bg-[#0f0f11] border border-white/5 rounded px-2 py-1.5 text-center shadow-[inset_0_2px_4px_rgba(0,0,0,0.5)] mb-6">
                <span class="text-sm text-zinc-200 font-mono tracking-wide">{formatFreqLabel(band.freq)}</span>
            </div>

            <div class="w-full flex items-center justify-between mb-4 pl-1">
                <div class="flex flex-col w-full mr-3">
                     <span class="text-[10px] font-bold uppercase text-zinc-600 mb-1">Gain</span>
                     <div class="bg-[#0f0f11] border border-white/5 rounded px-2 py-1 shadow-[inset_0_2px_4px_rgba(0,0,0,0.5)] flex justify-end">
                        <span 
                            class="text-xs font-mono"
                            style={`color: ${band.gain > 0 ? config.color : (band.gain < 0 ? '#a1a1aa' : '#52525b')}`}
                        >
                            {band.gain > 0 ? '+' : ''}{band.gain.toFixed(1)} <span class="text-[9px] text-zinc-700">dB</span>
                        </span>
                    </div>
                </div>
                 <Knob 
                    bind:value={band.gain} min={-15} max={15} step={0.1} 
                    size="sm"
                    color={config.color}
                    onChange={(val) => updateBand(i, 'gain', val)}
                 />
            </div>

            <div class="w-full flex items-center justify-between pl-1">
                <div class="flex flex-col w-full mr-3">
                     <span class="text-[10px] font-bold uppercase text-zinc-600 mb-1">Q</span>
                     <div class="bg-[#0f0f11] border border-white/5 rounded px-2 py-1 shadow-[inset_0_2px_4px_rgba(0,0,0,0.5)] flex justify-end">
                        <span class="text-xs font-mono" style={`color: ${config.color}`}>
                            {band.q.toFixed(2)}
                        </span>
                    </div>
                </div>
                 <Knob 
                    bind:value={band.q} min={0.1} max={10.0} step={0.1} 
                    size="sm"
                    color={config.color}
                    onChange={(val) => updateBand(i, 'q', val)}
                 />
            </div>
            
        </div>
      {/each}
    </div>
    {/if}
  </div>
</div>