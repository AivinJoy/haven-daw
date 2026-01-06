<!-- haven\src\lib\components\Timeline.svelte -->
<script lang="ts">
    import { createEventDispatcher } from 'svelte';
    import { ZoomIn, ZoomOut } from 'lucide-svelte';
    import { invoke } from '@tauri-apps/api/core';
    import DraggableTrackItem from './DraggableTrackItem.svelte';
    import ContextMenu from './ContextMenu.svelte';

    const dispatch = createEventDispatcher();

    let { tracks = $bindable([]), currentTime = 0, bpm = 120 } = $props();

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

      // --- CONTEXT MENU STATE ---
    let showMenu = $state(false);
    let menuPos = $state({ x: 0, y: 0 });
    let activeContext = $state<{ trackIndex: number; clipIndex: number } | null>(null);


    function handleClipContextMenu(event: CustomEvent, trackIndex: number, clipIndex: number) {
        const { x, y } = event.detail;
        // We only store the trackIndex. We will use 'currentTime' (Playhead) for the split.
        activeContext = { trackIndex, clipIndex };
        menuPos = { x, y }; 
        showMenu = true;
    }

    // --- CORE: Optimistic Split Logic ---
    async function executeSplit(trackIndex: number, splitTime: number) {
    if (trackIndex < 0 || trackIndex >= tracks.length) return;
    const track = tracks[trackIndex];
        
    const clipIndex = track.clips.findIndex((c: any) =>
      splitTime >= c.startTime && splitTime < c.startTime + c.duration
    );
    if (clipIndex === -1) return;
        
    const original = track.clips[clipIndex];
    const relative = splitTime - original.startTime;
        
    // Prevent edge splits (0 duration clips)
    if (relative <= 0.001 || relative >= original.duration - 0.001) return;
        
    const leftClip = { ...original, duration: relative };
    const rightClip = {
      ...original,
      id: `clip-${Date.now()}-split`,
      startTime: splitTime,
      offset: (original.offset ?? 0) + relative,
      duration: original.duration - relative
    };
  
    // Replace original with left, insert right after it
    track.clips.splice(clipIndex, 1, leftClip, rightClip);
  
    // Force reactivity
    tracks = [...tracks];
  
    try {
      await invoke("split_clip", { trackIndex, time: splitTime });
    } catch (e) {
      console.error("Backend split failed", e);
      // Optional: refresh from backend state if you want to rollback safely
    }
    }
    

    // --- KEYBOARD SHORTCUT (S Key) ---
    function handleKeyDown(e: KeyboardEvent) {
      const el = e.target as HTMLElement | null;
      const typing =
        el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable);
      if (typing) return;

      if (e.key.toLowerCase() === "s") {
        e.preventDefault();
        e.stopPropagation();
        tracks.forEach((t: any, idx: number) => {
          const hasClip = t.clips.some((c: any) => currentTime >= c.startTime && currentTime < c.startTime + c.duration);
          if (hasClip) executeSplit(idx, currentTime);
        });
      }
    }


    // --- CONTEXT MENU ACTION ---
    async function performSplit() {
        if (!activeContext) return;
        const { trackIndex } = activeContext;

        // FIX: Always use 'currentTime' (Playhead Position), ignore mouse click time.
        await executeSplit(trackIndex, currentTime);

        showMenu = false;
    }

    async function executeMergeNext(trackIndex: number, clipIndex: number) {
      const t = tracks[trackIndex];
      if (!t) return;
        
      const left = t.clips?.[clipIndex];
      const right = t.clips?.[clipIndex + 1];
      if (!left || !right) return;
        
      // Optimistic UI: extend left, remove right
      const merged = { ...left, duration: (left.duration ?? 0) + (right.duration ?? 0) };
      t.clips.splice(clipIndex, 2, merged);
      tracks = [...tracks];
        
      try {
        await invoke("merge_clip_with_next", { trackIndex, clipIndex });
      } catch (e) {
        console.error("Backend merge failed", e);
        dispatch("refresh"); // rollback by reloading backend state only on error
      }
    }


    async function performMergeNext() {
      if (!activeContext) return;
      const { trackIndex, clipIndex } = activeContext;

      showMenu = false;
      await executeMergeNext(trackIndex, clipIndex);
    }


    const EPS = 0.001;

    function canMergeNext(ctx: { trackIndex: number; clipIndex: number } | null) {
      if (!ctx) return false;

      const t = tracks[ctx.trackIndex];
      if (!t) return false;

      const left = t.clips?.[ctx.clipIndex];
      const right = t.clips?.[ctx.clipIndex + 1];
      if (!left || !right) return false;

      const leftEnd = (left.startTime ?? 0) + (left.duration ?? 0);

      const adjacentTimeline = Math.abs((right.startTime ?? 0) - leftEnd) <= EPS;
      const samePath = left.path === right.path;

      const leftSrcEnd = (left.offset ?? 0) + (left.duration ?? 0);
      const contiguousSource = Math.abs((right.offset ?? 0) - leftSrcEnd) <= EPS;

      return adjacentTimeline && samePath && contiguousSource;
    }



    async function handleClipMove(event: CustomEvent, clipIndex:number) {
        const { trackId, newStartTime } = event.detail;

        console.log(`🎵 Moving Track ${trackId} to ${newStartTime.toFixed(2)}s`);

        try {
            // Backend uses 0-based index, frontend uses 1-based ID
            // Ensure this matches your logic (track.id - 1)
            await invoke('move_clip', { 
                trackIndex: trackId - 1, 
                clipIndex: clipIndex,
                newTime: newStartTime 
            });
        } catch (e) {
            console.error("Failed to move clip:", e);
        }
    }

</script>

<svelte:window onmousemove={onScrubMove} onmouseup={stopScrub} on:keydown={handleKeyDown}/>
{#if showMenu}
    <ContextMenu
      x={menuPos.x}
      y={menuPos.y}
      onClose={() => (showMenu = false)}
      options={[
        { label: "Split Clip", action: performSplit },
        {
          label: "Merge with next",
          action: performMergeNext,
          disabled: !canMergeNext(activeContext)
        }
      ]}
    />

{/if}

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
                    {#each track.clips as clip, clipIndex (clip.id)}
                      <DraggableTrackItem
                        bind:clip={tracks[trackIndex].clips[clipIndex]}
                        zoom={zoomMultiplier}
                        {currentTime}
                        {bpm}
                        on:change={(e) => handleClipMove(e, clipIndex)}
                        on:contextmenu={(e) => handleClipContextMenu(e, trackIndex, clipIndex)}
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