<!-- haven\src\lib\components\Timeline.svelte -->
<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import { ZoomIn, ZoomOut } from 'lucide-svelte';
  import { invoke } from '@tauri-apps/api/core';
  import DraggableTrackItem from './DraggableTrackItem.svelte';

  const dispatch = createEventDispatcher();

  let { tracks = [], currentTime = 0, bpm = 120 } = $props();

  const PIXELS_PER_SECOND = 50; 

  interface GridLineData {
      time: number;
      is_bar_start: boolean;
      bar_number: number;
  }
  
  let zoomMultiplier = $state(1); 
  let gridLines: GridLineData[] = $state([]); 
  
  let rulerContainer: HTMLDivElement;
  let trackContainer: HTMLDivElement;
  let containerWidth = $state(0);

  // --- PLAYHEAD DRAG STATE ---
  let isScrubbing = false;

  // --- GRID ENGINE ---
  async function updateGrid() {
      if (!trackContainer || containerWidth === 0) return;

      const resolution = zoomMultiplier >= 0.67 ? 4 : 1;
      const scrollPx = trackContainer.scrollLeft;
      const startTime = Math.max(0, (scrollPx / zoomMultiplier / PIXELS_PER_SECOND) - 5);
      const endTime = ((scrollPx + containerWidth) / zoomMultiplier / PIXELS_PER_SECOND) + 5;

      try {
          gridLines = await invoke<GridLineData[]>('get_grid_lines', { 
              start: startTime, 
              end: endTime, 
              resolution: resolution 
          });
      } catch (e) {
          console.error("Grid error:", e);
      }
  }

  function handleScroll() {
      if (rulerContainer && trackContainer) {
          rulerContainer.scrollLeft = trackContainer.scrollLeft;
          updateGrid(); 
      }
  }

  $effect(() => {
      if (zoomMultiplier || bpm || containerWidth || tracks) {
          updateGrid();
          if (rulerContainer && trackContainer) {
              rulerContainer.scrollLeft = trackContainer.scrollLeft;
          }
      }
  });

  let maxDurationSeconds = $derived(
    Math.max(...tracks.map((t: any) => (t.startTime || 0) + (t.duration || 0)), 300)
  );

  function zoomIn() { zoomMultiplier = Math.min(zoomMultiplier * 1.5, 8); }
  function zoomOut() { zoomMultiplier = Math.max(zoomMultiplier / 1.5, 0.2); }

  // --- SEEKING LOGIC ---

  // 1. Click on Ruler (Jump)
  function handleRulerClick(event: MouseEvent) {
      if (!rulerContainer) return;
      const rect = rulerContainer.getBoundingClientRect();
      const clickX = event.clientX - rect.left + rulerContainer.scrollLeft;
      const time = clickX / (PIXELS_PER_SECOND * zoomMultiplier);
      
      dispatch('seek', Math.max(0, time));
  }

  // 2. Drag Playhead (Scrub)
  function startScrub(event: MouseEvent) {
      event.preventDefault();
      isScrubbing = true;
  }

  function onScrubMove(event: MouseEvent) {
      if (!isScrubbing || !trackContainer) return;
      
      const rect = trackContainer.getBoundingClientRect();
      // Calculate time based on mouse position relative to the scrolling container
      const offsetX = event.clientX - rect.left + trackContainer.scrollLeft;
      const time = Math.max(0, offsetX / (PIXELS_PER_SECOND * zoomMultiplier));
      
      // Dispatch immediately for smooth UI updates (optional: throttle this if backend lags)
      dispatch('seek', time);
  }

  function stopScrub() {
      isScrubbing = false;
  }

  function handleTrackClick(trackId: number) {
      dispatch('select', trackId);
  }

  async function handleClipMove(event: CustomEvent) {
      const { trackId, newStartTime } = event.detail;
      
      console.log(`🎵 Moving Track ${trackId} to ${newStartTime.toFixed(2)}s`);

      try {
          // Backend uses 0-based index, frontend uses 1-based ID
          // Ensure this matches your logic (track.id - 1)
          await invoke('set_track_start', { 
              trackIndex: trackId - 1, 
              startTime: newStartTime 
          });
      } catch (e) {
          console.error("Failed to move clip:", e);
      }
  }

</script>

<svelte:window onmousemove={onScrubMove} onmouseup={stopScrub} />

<div class="flex-1 h-full relative flex flex-col bg-[#13131f]/90 backdrop-blur-md overflow-hidden select-none">
  
  <div class="h-8 flex border-b border-white/10 bg-[#1a1a2e] shrink-0 z-20">
    <div 
        bind:this={rulerContainer} 
        class="flex-1 flex items-end overflow-hidden relative pb-1 cursor-pointer"
        onmousedown={handleRulerClick}
        role="button"
        tabindex="0"
    >
        <div class="h-full relative pointer-events-none" style="width: {maxDurationSeconds * PIXELS_PER_SECOND * zoomMultiplier}px;">
            
            {#each gridLines as line}
                <div 
                  class={`absolute bottom-0 border-l ${line.is_bar_start ? 'h-full border-white/30 z-10' : 'h-2 border-white/10 z-0'}`}
                  style="transform: translateX({line.time * PIXELS_PER_SECOND * zoomMultiplier}px);"
                >
                    {#if line.is_bar_start}
                        <span class="absolute top-0 left-1.5 text-[10px] text-white/70 font-mono font-bold select-none">
                            {line.bar_number}
                        </span>
                    {/if}
                </div>
            {/each}
            
            <div 
                class="absolute top-0 bottom-0 w-0 border-l border-red-500 z-20"
                style="transform: translateX({currentTime * PIXELS_PER_SECOND * zoomMultiplier}px);"
            >
                <div class="absolute top-0 -left-[5px] w-0 h-0 border-l-[5px] border-l-transparent border-r-[5px] border-r-transparent border-t-8 border-t-red-500"></div>
            </div>
        </div>
    </div>
    
    <div class="flex items-center border-l border-white/10 px-1 bg-[#151520]">
        <button onclick={zoomOut} class="p-1.5 text-white/40 hover:text-white rounded"><ZoomOut size={14} /></button>
        <button onclick={zoomIn} class="p-1.5 text-white/40 hover:text-white rounded"><ZoomIn size={14} /></button>
    </div>
  </div>

  <div 
      bind:this={trackContainer} 
      bind:clientWidth={containerWidth}
      onscroll={handleScroll}
      class="flex-1 relative overflow-auto custom-scrollbar"
  >
    <div class="relative" style="width: {maxDurationSeconds * PIXELS_PER_SECOND * zoomMultiplier}px; min-height: 100%;">

        <div class="absolute inset-0 flex pointer-events-none h-full">
            {#each gridLines as line}
              <div 
                class={`absolute top-0 bottom-0 w-px ${line.is_bar_start ? 'bg-white/10' : 'bg-white/5'}`}
                style="transform: translateX({line.time * PIXELS_PER_SECOND * zoomMultiplier}px);"
              ></div>
            {/each}

        </div>

        <div class="absolute inset-0 flex flex-col pt-4 px-0"> 
            {#each tracks as track, trackIndex}
                <div class="w-full h-24 mb-2 relative border-b border-white/5 flex items-center px-0"
                    onmousedown={() => handleTrackClick(track.id)}
                    role="button"
                    tabindex="0"
                >    
                    <div class={`absolute inset-0 transition-colors duration-300 ${track.color}`} style={`opacity: ${track.isRecording ? 0.08 : 0};`}></div>
                    {#each track.clips as clip, clipIndex}
                        <DraggableTrackItem 
                            bind:clip={tracks[trackIndex].clips[clipIndex]} 
                            zoom={zoomMultiplier} 
                            currentTime={currentTime}
                            bpm={bpm}
                            on:change={handleClipMove} 
                        />
                    {/each}    
                </div>
            {/each}
        </div>

        <div 
            class="absolute top-0 bottom-0 w-4 -ml-2 z-30 cursor-ew-resize group flex justify-center"
            style="transform: translateX({currentTime * PIXELS_PER_SECOND * zoomMultiplier}px);"
            onmousedown={startScrub}
            role="slider"
            tabindex="0"
            aria-valuenow={currentTime}
        >
            <div class="w-px h-full bg-white shadow-[0_0_10px_rgba(255,255,255,0.5)] group-hover:bg-red-400 group-active:bg-red-500"></div>
        </div>

    </div>
  </div>
</div>

<style>
    .custom-scrollbar::-webkit-scrollbar { width: 10px; height: 10px; }
    .custom-scrollbar::-webkit-scrollbar-track { background: #0f0f16; border-left: 1px solid rgba(255,255,255,0.05); }
    .custom-scrollbar::-webkit-scrollbar-thumb { background: rgba(255,255,255,0.1); border-radius: 5px; border: 2px solid #0f0f16; }
    .custom-scrollbar::-webkit-scrollbar-thumb:hover { background: rgba(255,255,255,0.2); }
</style>