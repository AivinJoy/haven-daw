<!-- haven\src\lib\components\TrackControl.svelte -->
<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { Mic, Headphones, MoreVertical, Volume2, Ear } from 'lucide-svelte';

  // --- PROPS ---
  let { 
    track = $bindable(), 
    index,
    id,
    name = $bindable(),
    color,
    gain = $bindable(),
    pan = $bindable(),
    muted = $bindable(),
    solo = $bindable(),
    isRecording = false,
    source = 'media',
    monitor = false,
    onmonitor = () => {},
    onmenu = (e: MouseEvent) => {}
  } = $props();

  // --- LOCAL STATE ---
  let volumeSlider = $state(getSliderValue(gain));
  let visualPan = $state(pan * 50);

  $effect(() => {
      volumeSlider = getSliderValue(gain);
      visualPan = pan * 50;
  });
  
  // --- VOLUME LOGIC (75% = 1.0 Gain) ---
  function getSliderValue(gain: number) {
      return Math.min(100, gain * 75);
  }

  function fromSliderValue(val: number) {
      return val / 75.0;
  }

  // --- COLOR MAPPING ---
  const colorMap: Record<string, string> = {
    'bg-brand-blue': '#3b82f6',
    'bg-brand-red': '#ef4444',
    'bg-purple-500': '#a855f7',
    'bg-emerald-500': '#10b981',
    'bg-orange-500': '#f97316',
    'bg-pink-500': '#ec4899',
    // --- FIX: Added missing colors so they don't default to Blue ---
    'bg-cyan-500': '#06b6d4',
    'bg-indigo-500': '#6366f1',
    'bg-rose-500': '#f43f5e'
    // -------------------------------------------------------------
  };
  let trackColorHex = $derived(colorMap[color] || '#3b82f6');

  // --- BACKEND ACTIONS ---

  function updateVolume(e: Event) {
      const val = parseFloat((e.target as HTMLInputElement).value);
      volumeSlider = val;
      gain = fromSliderValue(val);
      invoke('set_track_gain', { trackIndex: index, gain: gain });
  }

  function resetVolume() {
      volumeSlider = 75; // Visual 75%
      gain = 1.0; // Actual 1.0
      invoke('set_track_gain', { trackIndex: index, gain: 1.0 });
  }

  function toggleMute() {
      muted = !muted;
      invoke('toggle_mute', { trackIndex: index });
  }

  function toggleSolo() {
      solo = !solo;
      invoke('toggle_solo', { trackIndex: index });
  }

  function resetPan() {
      visualPan = 0;
      pan = 0.0;
      invoke('set_track_pan', { trackIndex: index, pan: 0.0 });
  }

  // --- KNOB LOGIC (PAN) ---
  let isDraggingKnob = false;
  let startY = 0;
  let startVisualPan = 0;
  const DRAG_SENSITIVITY = 3; 

  function startDrag(e: MouseEvent) {
    e.preventDefault(); 
    isDraggingKnob = true;
    startY = e.clientY;
    startVisualPan = visualPan; 
    window.addEventListener('mousemove', handleDrag);
    window.addEventListener('mouseup', stopDrag);
  }

  function handleDrag(e: MouseEvent) {
    if (!isDraggingKnob) return;
    const deltaY = startY - e.clientY;
    
    let newPos = startVisualPan + (deltaY / DRAG_SENSITIVITY);
    if (newPos > -2 && newPos < 2) newPos = 0;
    visualPan = Math.max(-50, Math.min(50, newPos));

    pan = visualPan / 50.0; 
    invoke('set_track_pan', { trackIndex: index, pan: pan });
  }

  function stopDrag() {
    isDraggingKnob = false;
    window.removeEventListener('mousemove', handleDrag);
    window.removeEventListener('mouseup', stopDrag);
  }
</script>

{#snippet MusicIconType({ src }: { src: string })}
    {#if src === 'mic'}
        <Mic size={14} style="color: {trackColorHex}" />
    {:else}
        <Headphones size={14} style="color: {trackColorHex}" />
    {/if}
{/snippet}

<div class="group relative w-full h-full glass-panel border-l-[3px] rounded-lg border-l-transparent hover:bg-white/5 transition-all mb-2 flex flex-col justify-center px-3 gap-2 overflow-hidden shrink-0 shadow-[0_4px_20px_rgba(0,0,0,0.3)]">
  
  <div class={`absolute left-0 top-0 bottom-0 w-1 ${color} opacity-80 shadow-[0_0_15px_${color.replace('bg-', '')}]`}></div>

  <div class="flex items-center w-full gap-3 pl-2">
    <span class="text-white/30 font-mono text-[10px] select-none shrink-0">{id.toString().padStart(2, '0')}</span>
    
    <div class="opacity-80 shrink-0">
        {@render MusicIconType({ src: source })}
    </div>

    <input 
        type="text" 
        bind:value={name} 
        class="bg-transparent border-none text-white/90 text-sm font-bold flex-1 min-w-0 focus:ring-0 p-0 placeholder-white/20 focus:outline-none"
    />

    <div class="flex items-center gap-1 shrink-0 ml-auto">
        {#if source === 'mic'}
            <button 
                onclick={() => onmonitor()} 
                class={`w-6 h-6 rounded flex items-center justify-center border transition-all ${monitor ? 'bg-emerald-500/20 border-emerald-500 text-emerald-500 shadow-[0_0_8px_rgba(16,185,129,0.4)]' : 'border-white/10 text-white/40 hover:border-white/30 hover:text-white/70'}`}
                title="Input Monitor (Hear yourself)"
            >
                <Ear size={14} />
            </button>
        {/if}
        <button 
            onclick={toggleMute}
            class={`w-6 h-6 rounded text-[9px] font-bold border transition-all ${muted ? 'bg-red-500/20 border-red-500 text-red-500 shadow-[0_0_8px_rgba(220,38,38,0.4)]' : 'border-white/10 text-white/40 hover:border-white/30 hover:text-white/70'}`}
        >M</button>
        
        <button 
            onclick={toggleSolo}
            class={`w-6 h-6 rounded text-[9px] font-bold border transition-all ${solo ? 'bg-yellow-500/20 border-yellow-500 text-yellow-500 shadow-[0_0_8px_rgba(234,179,8,0.4)]' : 'border-white/10 text-white/40 hover:border-white/30 hover:text-white/70'}`}
        >S</button>
        
        <button 
            onclick={(e) => onmenu(e)} 
            class="w-6 h-6 flex items-center justify-center text-white/20 hover:text-white transition-colors" 
            aria-label="Track Settings"
        >
            <MoreVertical size={14} />
        </button>
    </div>
  </div>

  <div class="flex items-center justify-between w-full pl-2 pr-1">
    
    <div class="flex items-center gap-3">
        <Volume2 size={14} class="text-white/30 shrink-0" />

        <input 
            type="range" min="0" max="100" 
            value={volumeSlider}
            oninput={updateVolume}
            ondblclick={resetVolume}
            style="background: linear-gradient(to right, {trackColorHex} 0%, {trackColorHex} {volumeSlider}%, rgba(255,255,255,0.1) {volumeSlider}%, rgba(255,255,255,0.1) 100%);"
            class="w-28 h-1 rounded-lg appearance-none cursor-pointer [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:w-3 [&::-webkit-slider-thumb]:h-3 [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-white/80 [&::-webkit-slider-thumb]:shadow-[0_0_5px_white] hover:[&::-webkit-slider-thumb]:bg-white"
        />
    </div>

    <div class="flex flex-col items-center justify-center gap-1 w-10 relative shrink-0">
      
      <div class="relative w-6 h-6 flex items-center justify-center">
        <div class="absolute inset-0 flex items-center justify-center pointer-events-none z-20">
          {#each [-135, -90, -45, 0, 45, 90, 135] as degree}
            <div
              class="absolute origin-center"
              style:transform={`rotate(${degree}deg) translateY(-11px)`}
            >
              <div class={`w-0.5 h-0.5 rounded-full ${degree === 0 ? 'bg-white shadow-[0_0_4px_white]' : 'bg-white/30'}`}></div>
            </div>
          {/each}
        </div>
      
        <div
          class="relative w-5 h-5 rounded-full bg-[#1e1e28] border border-white/10 shadow-lg flex items-center justify-center cursor-ns-resize hover:bg-[#252530] hover:border-white/30 transition-all z-10 outline-none"
          style:transform={`rotate(${visualPan * 2.7}deg)`}
          role="slider"
          tabindex="0"
          aria-label="Pan Control"
          aria-valuenow={visualPan}
          aria-valuemin="-50"
          aria-valuemax="50"
          onmousedown={startDrag}
          ondblclick={resetPan}
        >
          <div
            class="absolute top-0.5 w-0.5 h-1.5 rounded-full transition-colors duration-200"
            style:background-color={Math.abs(visualPan) < 2 ? 'white' : trackColorHex}
            style:box-shadow={Math.abs(visualPan) < 2 ? '0 0 5px white' : `0 0 5px ${trackColorHex}`}
          ></div>
        </div>
      </div>
    
      <div class="flex justify-between w-full text-[9px] font-bold font-sans text-white/40 select-none -mt-1 relative z-30 px-0.5">
        <span style:color={visualPan < -40 ? trackColorHex : ''}>L</span>
        <span style:color={visualPan > 40 ? trackColorHex : ''}>R</span>
      </div>
    </div>

  </div>

</div>