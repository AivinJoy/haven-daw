<!-- haven\src\lib\components\Timeline.svelte -->
<script lang="ts">
    import { createEventDispatcher } from 'svelte';
    import { ZoomIn, ZoomOut } from 'lucide-svelte';
    import { invoke } from '@tauri-apps/api/core';
    import DraggableTrackItem from './DraggableTrackItem.svelte';
    import ContextMenu from './ContextMenu.svelte';
    import AutomationLane from './AutomationLane.svelte';
    import { ui } from '$lib/stores/ui.svelte';

    const dispatch = createEventDispatcher();

    let { tracks = $bindable([]), 
        currentTime = 0, bpm = 120, 
        timeSignatureNumerator = 4,
        timeSignatureDenominator = 4
    } = $props();

    const PIXELS_PER_SECOND = 50; 

    interface GridLineData {
        time: number;
        is_bar_start: boolean;
        bar_number: number;
    }

    let zoomMultiplier = $state(1); 
    let scrollLeft = $state(0);

    let rulerContainer: HTMLDivElement;
    let trackContainer: HTMLDivElement;
    let containerWidth = $state(0);

    // --- PLAYHEAD DRAG STATE ---
    let isScrubbing = false;

    // --- AUTOSCROLL ENGINE ---
    let isAutoScrollSuspended = $state(false);
    let autoScrollTimeout: ReturnType<typeof setTimeout>;
    let expectedScrollLeft = -1;
    let isScrollThrottled = false; // NEW: rAF lock for scroll events

    function suspendAutoScroll() {
        isAutoScrollSuspended = true;
        clearTimeout(autoScrollTimeout);
        autoScrollTimeout = setTimeout(() => {
            isAutoScrollSuspended = false;
        }, 2000); // Wait 2 seconds before resuming auto-scroll
    }
    
    // Watch currentTime and auto-scroll if it leaves the viewport
    // Watch currentTime and auto-scroll if it leaves the viewport
    $effect(() => {
        if (!trackContainer || isScrubbing || containerWidth === 0 || isAutoScrollSuspended) return;

        const playheadPx = currentTime * PIXELS_PER_SECOND * zoomMultiplier;
        const currentScrollPx = trackContainer.scrollLeft; // ✅ Renamed to avoid collision
        
        let targetScroll = -1;

        // Page scroll right when playhead leaves viewport
        if (playheadPx > currentScrollPx + containerWidth) {
            targetScroll = playheadPx - 50; // Leave a 50px left margin
        } 
        // Page scroll left when playhead moves backwards out of viewport
        else if (playheadPx < currentScrollPx && currentScrollPx > 0) {
            targetScroll = Math.max(0, playheadPx - 50);
        }
        
        if (targetScroll !== -1) {
            expectedScrollLeft = targetScroll; 
            trackContainer.scrollLeft = targetScroll;
            if (rulerContainer) rulerContainer.scrollLeft = targetScroll;
            
            // Sync our reactive state so the grid updates instantly during the jump
            scrollLeft = targetScroll; 
        }
    });

    // --- GRID ENGINE (LOCAL DETERMINISTIC MATH) ---
    // Performance Trackers for Spatial Throttling
    let cachedGrid: GridLineData[] = [];
    let lastCalcScroll = -1;
    let lastCalcZoom = -1;
    let lastCalcWidth = -1;
    const SPATIAL_THRESHOLD = 50; // pixels

    let gridLines = $derived.by(() => {
        if (containerWidth === 0 || bpm <= 0) return [];

        // 1. Spatial Throttling: Reuse grid if movement is small and constraints are unchanged
        if (
            cachedGrid.length > 0 &&
            lastCalcZoom === zoomMultiplier &&
            lastCalcWidth === containerWidth &&
            Math.abs(scrollLeft - lastCalcScroll) < SPATIAL_THRESHOLD
        ) {
            return cachedGrid;
        }

        // 2. Setup Deterministic Math Variables (1:1 with Rust backend)
        const secondsPerQuarterNote = 60 / bpm;
        const quartersPerBar = timeSignatureNumerator * (4 / timeSignatureDenominator);
  
        // 3. Dynamic Resolution based on standard musical divisions
        let resolution = 1; // Default to Bars
        if (zoomMultiplier > 2.0) resolution = 16; // 16th notes
        else if (zoomMultiplier >= 0.67) resolution = timeSignatureDenominator; // e.g. 8th notes in 6/8

        // Calculate step timing in quarters
        const quartersPerStep = (resolution === 1) ? quartersPerBar : (4 / resolution);
        const timeStep = quartersPerStep * secondsPerQuarterNote;
        const stepsPerBar = Math.round(quartersPerBar / quartersPerStep);

        // 4. Render Buffer (1 full screen before and after viewport)
        const bufferPixels = containerWidth;
        const startPx = Math.max(0, scrollLeft - bufferPixels);
        const endPx = scrollLeft + containerWidth + bufferPixels;

        // 5. Convert Pixels to Time (Exact match to playhead formula for perfect sync)
        const startTime = startPx / (PIXELS_PER_SECOND * zoomMultiplier);
        const endTime = endPx / (PIXELS_PER_SECOND * zoomMultiplier);

        const startStep = Math.floor(startTime / timeStep);
        let endStep = Math.ceil(endTime / timeStep);

        // 6. Performance Protection: Max Line Cap to prevent DOM freezing on massive zoom-outs
        const MAX_GRID_LINES = 2000;
        if (endStep - startStep > MAX_GRID_LINES) {
            endStep = startStep + MAX_GRID_LINES;
        }

        // 7. Generate Grid Data
        const lines: GridLineData[] = [];
        for (let i = startStep; i <= endStep; i++) {
            lines.push({
                time: i * timeStep,
                // Safely handle divisions to prevent modulo-by-zero or NaN
                is_bar_start: stepsPerBar === 0 ? true : i % stepsPerBar === 0,
                bar_number: stepsPerBar === 0 ? i + 1 : Math.floor(i / stepsPerBar) + 1
            });
        }

        // Cache results for the next scroll threshold check
        lastCalcScroll = scrollLeft;
        lastCalcZoom = zoomMultiplier;
        lastCalcWidth = containerWidth;
        cachedGrid = lines;

        return lines;
    });

    function handleScroll() {
        // GUARD: If a frame is already queued, ignore the barrage of incoming scroll events
        if (isScrollThrottled) return;
        
        isScrollThrottled = true;

        requestAnimationFrame(() => {
            if (!trackContainer || !rulerContainer) {
                isScrollThrottled = false;
                return;
            }
            
            // 1. Visually sync Ruler (Master-Slave architecture)
            rulerContainer.scrollLeft = trackContainer.scrollLeft;
            
            // 2. Update reactive state (Synchronously triggers Grid Math if outside threshold)
            scrollLeft = trackContainer.scrollLeft;

            // 🛡️ ROBUST SYNC CHECK
            // Allow a 2px tolerance for browser sub-pixel scrolling differences
            if (expectedScrollLeft !== -1 && Math.abs(trackContainer.scrollLeft - expectedScrollLeft) <= 2) {
                // This scroll was generated by the auto-scroll engine.
            } else {
                // The scroll differs from our expected target -> The user is manually scrolling
                suspendAutoScroll();
                expectedScrollLeft = trackContainer.scrollLeft;
            }

            // Release the lock for the next frame
            isScrollThrottled = false;
        });
    }

    $effect(() => {
        // Visual ruler sync on zoom or container resize
        // (Grid automatically updates via the $derived block)
        if (zoomMultiplier || containerWidth) {
            if (rulerContainer && trackContainer) {
                rulerContainer.scrollLeft = trackContainer.scrollLeft;
                scrollLeft = trackContainer.scrollLeft;
            }
        }
    });

    // FIX: Extract true duration from clips and ensure timeline always has 60s of runway ahead
    let maxDurationSeconds = $derived(
      Math.max(
          ...tracks.flatMap((t: any) => t.clips ? t.clips.map((c: any) => c.startTime + c.duration) : [0]),
          currentTime + 60, 
          300
      )
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

        isAutoScrollSuspended = false; // Instantly resume auto-scroll on manual seek
        dispatch('seek', Math.max(0, time));
    }

    // 2. Drag Playhead (Scrub)
    function startScrub(event: MouseEvent) {
        event.preventDefault();

        isScrubbing = true;
        isAutoScrollSuspended = false; // Instantly resume auto-scroll when picking up playhead
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
          await invoke("split_clip", { 
            trackId: track.id, 
            time: splitTime 
        });
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
        
        // 1. Find the currently selected (armed) track index
        const selectedIdx = tracks.findIndex((t: any) => t.isRecording);
        
        // 2. Only split if a track is selected and has a valid clip at the playhead
        if (selectedIdx !== -1) {
             const t = tracks[selectedIdx];
             const hasClip = t.clips.some((c: any) => currentTime >= c.startTime && currentTime < c.startTime + c.duration);
             
             if (hasClip) {
                 executeSplit(selectedIdx, currentTime);
             }
        } else {
            console.warn("No track selected for splitting. Click a track header to select it.");
        }
      }
    }

    // --- AUTOMATION KEYBOARD SHORTCUT (A Key) ---
    function handleAutomationShortcut(e: KeyboardEvent) {
        const el = e.target as HTMLElement | null;
        const typing = el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable);
        if (typing) return;

        if (e.key.toLowerCase() === "a" && !e.ctrlKey && !e.metaKey) {
            e.preventDefault();
            e.stopPropagation();
            ui.toggleAutomation();
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
        await invoke("merge_clip_with_next", { 
            trackId: t.id, 
            clipIndex 
        });
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

    // --- NEW: Delete Logic ---
    async function performDelete() {
        if (!activeContext) return;
        const { trackIndex, clipIndex } = activeContext;

        // 1. Optimistic UI Update
        if (tracks[trackIndex] && tracks[trackIndex].clips) {
            tracks[trackIndex].clips.splice(clipIndex, 1);
            tracks = [...tracks]; // Trigger Reactivity
        }
        
        showMenu = false;

        // 2. Sync with Backend
        try {
            await invoke('delete_clip', { 
                trackId: tracks[trackIndex].id, 
                clipIndex 
            });
        } catch (e) {
            console.error("Failed to delete clip:", e);
            dispatch("refresh"); // Fallback: reload state from backend on error
        }
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
                trackId: trackId, 
                clipIndex: clipIndex,
                newTime: newStartTime 
            });
        } catch (e) {
            console.error("Failed to move clip:", e);
        }
    }

</script>

<svelte:window 
    onmousemove={onScrubMove} 
    onmouseup={stopScrub} 
    on:keydown={(e) => {
        handleKeyDown(e);
        handleAutomationShortcut(e);
    }}
/>

{#if showMenu}
    <ContextMenu
      x={menuPos.x}
      y={menuPos.y}
      onClose={() => (showMenu = false)}
      options={[
        { label: "Split Clip",
          action: performSplit 
        },

        {
          label: "Merge with next",
          action: performMergeNext,
          disabled: !canMergeNext(activeContext)
        },
        // NEW: Delete Option
        { 
          label: "Delete Clip", 
          action: performDelete, 
          danger: true 
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
            onwheel={(e) => e.preventDefault()}
            role="button"
            tabindex="0"
        >
            <div class="h-full relative pointer-events-none shrink-0" style="width: {maxDurationSeconds * PIXELS_PER_SECOND * zoomMultiplier}px; min-width: max-content;">

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
                    style="transform: translateX({currentTime * PIXELS_PER_SECOND * zoomMultiplier}px); will-change: transform;"
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
        <div class="relative shrink-0" style="width: {maxDurationSeconds * PIXELS_PER_SECOND * zoomMultiplier}px; min-height: 100%; min-width: max-content;">

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
                        {#if ui.showAutomation}
                            <AutomationLane 
                                trackId={track.id}
                                width={maxDurationSeconds * PIXELS_PER_SECOND * zoomMultiplier} 
                                height={96} 
                                pixelsPerSecond={PIXELS_PER_SECOND * zoomMultiplier} 
                            />
                        {/if}
                    </div>
                {/each}
            </div>
            <div 
                class="absolute top-0 bottom-0 w-4 -ml-2 z-30 cursor-ew-resize group flex justify-center"
                style="transform: translateX({currentTime * PIXELS_PER_SECOND * zoomMultiplier}px); will-change: transform;"
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