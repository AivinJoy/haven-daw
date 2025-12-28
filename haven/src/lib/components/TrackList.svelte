<!-- haven\src\lib\components\TrackList.svelte -->
<script lang="ts">
  import { Plus } from 'lucide-svelte';
  import TrackControl from './TrackControl.svelte';
  import { createEventDispatcher } from 'svelte';

  const dispatch = createEventDispatcher<{
    requestAdd: null;        // No payload
    select: number;          // Payload is Track ID
    toggleMonitor: number;   // Payload is Track ID
  }>();

  // Receive tracks prop (bindable if you want the list itself to change, but usually internal props bind)
  let { tracks = []} = $props();

  function requestAddTrack() {
    dispatch('requestAdd', null);
  }


  // Helper to handle click
  function selectTrack(id: number, e: MouseEvent | KeyboardEvent) {
    // 1. STOP if clicking a button or slider (Input/Button)
    // This prevents the row selection from interfering with Mute/Solo/Pan dragging
    const target = e.target as HTMLElement;
    if (target.tagName === 'BUTTON' || target.tagName === 'INPUT' || target.closest('button') || target.closest('input')) {
        return;
    }

    dispatch('select', id);
  }
</script>

<div class="w-[320px] h-full flex flex-col border-r border-white/10 bg-[#0a0a0f]/60 backdrop-blur-xl relative z-10">
  
  <div class="h-8 flex items-center justify-between px-4 border-b border-white/5 shadow-[0_4px_20px_rgba(0,0,0,0.2)]">
    <span class="text-xs font-bold tracking-widest text-white/60 glow-text-blue">TRACKS</span>
    
    <button 
        onclick={requestAddTrack}
        class="h-6 px-3 rounded-full bg-brand-blue/10 border border-brand-blue/30 flex items-center gap-1.5 text-brand-blue hover:bg-brand-blue/20 hover:border-brand-blue/60 transition-all shadow-[0_0_10px_rgba(59,130,246,0.2)] group"
    >
        <Plus size={13} class="group-hover:scale-110 transition-transform" />
        <span class="text-[10px] font-bold uppercase tracking-wider">Add Track</span>
    </button>
  </div>

  <div class="flex-1 overflow-y-auto pt-4 px-4 scrollbar-hide">
    {#each tracks as track, i (track.id)}
        <div 
            class={`w-full h-24 mb-2 rounded-xl transition-all cursor-pointer relative overflow-hidden outline-none ${
                track.isRecording 
                ? 'bg-white/5 border-transparent' // SELECTED: Subtle background, no border conflict
                : 'border-transparent hover:bg-white/5 hover:border-white/5' // UNSELECTED
            }`}
            onclick={(e) => selectTrack(track.id, e)}
            role="button"
            tabindex="0"
            onkeydown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault(); 
                    selectTrack(track.id, e);
                }
            }}
        >
            {#if track.isRecording}
                <div class="absolute left-0 top-0 bottom-0 w-1  shadow-[0_0_10px_#ef4444]"></div>
            {/if}

            <TrackControl 
                index={i} 
                id={track.id}
                name={track.name}
                color={track.color}
                
                bind:gain={tracks[i].gain}
                bind:pan={tracks[i].pan}
                bind:muted={tracks[i].muted}
                bind:solo={tracks[i].solo}
                isRecording={track.isRecording}
                source={track.source}

                monitor={track.monitor}
                onmonitor={() => dispatch('toggleMonitor', track.id)}
            />
        </div>
    {/each}
  </div>
</div>

<style>
    .scrollbar-hide::-webkit-scrollbar { display: none; }
    .scrollbar-hide { -ms-overflow-style: none; scrollbar-width: none; }
    .glow-text-blue { text-shadow: 0 0 10px rgba(59, 130, 246, 0.5); }
</style>