<!-- haven\src\lib\components\DraggableTrackItem.svelte -->
<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { createEventDispatcher, onMount } from 'svelte';
  import WaveformClip from './WaveformClip.svelte';

  // Props
  // We use $bindable() for 'track' so the parent list updates instantly visually
  let { 
    clip = $bindable(), 
    zoom = 1,
    currentTime = 0,
    bpm = 120
  } = $props();

  const PIXELS_PER_SECOND = 50;
  const dispatch = createEventDispatcher();

  // Local Drag State
  let isDragging = $state(false);
  let startMouseX = 0;      // Where the mouse was on screen (pixels)
  let initialStartTime = 0; // Where the track was in time (seconds)

  function onMouseDown(event: MouseEvent) {
    if (event.button !== 0) return; // Only left click
    
    event.preventDefault(); 
    event.stopPropagation(); 

    isDragging = true;
    
    // 1. Capture the starting state
    startMouseX = event.clientX; 
    initialStartTime = clip.startTime || 0;
    
    // 2. Add listeners globally (fixes dragging outside the div)
    window.addEventListener('mousemove', handleMouseMove);
    window.addEventListener('mouseup', handleMouseUp);
  }

  function handleMouseMove(event: MouseEvent) {
    if (!isDragging) return;

    // A. Calculate Delta
    const deltaPx = event.clientX - startMouseX;
    const deltaSecs = deltaPx / (PIXELS_PER_SECOND * zoom);
    
    // B. Calculate Raw Time
    let newTime = Math.max(0, initialStartTime + deltaSecs);

    // C. Grid Snapping (Musical Time)
    // C. Grid Snapping (PIXEL-ALIGNED)
    if (!event.shiftKey) {
        const beatDuration = 60 / bpm;
        const beatPx = beatDuration * PIXELS_PER_SECOND * zoom;
    
        // current pixel position
        const rawPx = newTime * PIXELS_PER_SECOND * zoom;
    
        // snap in pixel space
        const snappedPx = Math.round(rawPx / beatPx) * beatPx;
    
        // convert back to time
        newTime = snappedPx / (PIXELS_PER_SECOND * zoom);
    }

    
    // D. Update Audio State (High Precision Float)
    clip.startTime = newTime;
  }

  function handleRightClick(e: MouseEvent) {
        e.preventDefault(); // Stop browser menu
        dispatch('contextmenu', {
            x: e.clientX,
            y: e.clientY,
            // We can pass specific clip info if needed, 
            // but the parent loop usually knows the index.
        });
  }

  async function handleMouseUp() {
    if (isDragging) {
          isDragging = false;
          window.removeEventListener('mousemove', handleMouseMove);
          window.removeEventListener('mouseup', handleMouseUp);
          
          // --- NEW: Dispatch Change Event ---
          // This tells the parent "I moved to this new time!"
          dispatch('change', { 
              trackId: clip.trackId,
              clipId: clip.id, 
              newStartTime: clip.startTime 
          });
      }
  }

  let leftPx = $derived((clip.startTime || 0) * PIXELS_PER_SECOND * zoom);
  let widthPx = $derived((clip.duration || 0) * PIXELS_PER_SECOND * zoom);
</script>

<div 
    class="absolute h-full flex items-center cursor-grab active:cursor-grabbing hover:brightness-110 transition-filter"
    style="
        transform: translateX({leftPx}px);
        width: {widthPx}px;
        z-index: {isDragging ? 50 : 10}; 
        will-change: transform;
    "
    onmousedown={onMouseDown}
    oncontextmenu={handleRightClick}
    role="button"
    tabindex="0"
>
     <WaveformClip 
        color={clip.color} 
        waveform={clip.waveform} 
        currentTime={currentTime}
        startTime={clip.startTime || 0} 
        duration={clip.duration}
        offset={clip.offset || 0}
        zoom={zoom} 
        name={clip.name}
        clipNumber={clip.clipNumber}
     />
</div>