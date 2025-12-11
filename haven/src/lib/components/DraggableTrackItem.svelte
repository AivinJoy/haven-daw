<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import WaveformClip from './WaveformClip.svelte';

  // Props
  // We use $bindable() for 'track' so the parent list updates instantly visually
  let { 
    track = $bindable(), 
    index,
    zoom,
    currentTime
  } = $props();

  const PIXELS_PER_SECOND = 50;

  // Local Drag State
  let isDragging = $state(false);
  let dragStartX = 0;
  let dragStartTime = 0;

  function onMouseDown(event: MouseEvent) {
    if (event.button !== 0) return; // Only left click
    
    event.preventDefault(); // Prevent text selection
    event.stopPropagation(); // Stop bubbling

    isDragging = true;
    dragStartX = event.clientX;
    dragStartTime = track.startTime || 0;
  }

  function onWindowMouseMove(event: MouseEvent) {
    if (!isDragging) return;

    const deltaPx = event.clientX - dragStartX;
    const deltaSecs = deltaPx / (PIXELS_PER_SECOND * zoom);
    
    // Calculate new start time (cannot be less than 0)
    let newTime = Math.max(0, dragStartTime + deltaSecs);
    
    // Update visual state immediately
    track.startTime = newTime;
  }

  async function onWindowMouseUp() {
    if (!isDragging) return;
    
    isDragging = false;

    // Sync with Rust Backend
    try {
        await invoke('set_track_start', { 
            trackIndex: index, 
            startTime: track.startTime 
        });
        console.log(`Moved Track ${index} to ${track.startTime.toFixed(3)}s`);
    } catch (e) {
        console.error("Failed to move track:", e);
    }
  }
</script>

<svelte:window onmousemove={onWindowMouseMove} onmouseup={onWindowMouseUp} />

<div 
    class="absolute h-full flex items-center cursor-grab active:cursor-grabbing hover:brightness-110 transition-filter"
    style="
        left: {(track.startTime || 0) * PIXELS_PER_SECOND * zoom}px;
        width: {(track.duration || 0) * PIXELS_PER_SECOND * zoom}px;
        z-index: {isDragging ? 50 : 10}; 
    "
    onmousedown={onMouseDown}
    role="button"
    tabindex="0"
>
     <WaveformClip 
        color={track.color} 
        waveform={track.waveform} 
        currentTime={currentTime}
        startTime={track.startTime || 0} 
        duration={track.duration}
        zoom={zoom} 
        name={track.name}
     />
</div>