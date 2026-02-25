<!-- haven\src\lib\components\TopToolbar.svelte -->
<script lang="ts">
    import { 
      Menu, Undo, Redo, Timer, Play, Pause, Circle, Square, 
      SlidersHorizontal, Volume2, Save, Share, SkipBack 
    } from 'lucide-svelte';
    import { createEventDispatcher, onMount, onDestroy } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { save, open as openDialog } from '@tauri-apps/plugin-dialog';
    import MenuDropdown from './MenuDropdown.svelte';

    // 1. Accept Props
    let { 
      isPlaying = false, 
      isRecording = false, 
      currentTime = 0, 
      bpm = $bindable(120),
      masterGain = $bindable(1.0) 
    } = $props();

    const dispatch = createEventDispatcher();

    // Local state
    let timeSignature = $state('4 / 4');


    // Maps a true dB value to a UI Percentage (0-100)
    function dbToPercent(db: number) {
        if (db <= -60) return 0;
        if (db <= 0) {
            return ((db + 60) / 60) * 80; // Bottom 80% covers -60dB to 0dB
        } else {
            return 80 + (db / 6) * 20;    // Top 20% covers 0dB to +6dB
        }
    }

    // Maps a UI Percentage (0-100) back to a true dB value
    function percentToDb(percent: number) {
        if (percent <= 0) return -60;
        if (percent <= 80) {
            return (percent / 80) * 60 - 60;
        } else {
            return ((percent - 80) / 20) * 6;
        }
    }
    
    let masterVolume = $state(80); // 0.0dB now sits perfectly at 80%
    $effect(() => {
        let currentDb = masterGain <= 0.00001 ? -60 : 20 * Math.log10(masterGain);
        masterVolume = Math.max(0, Math.min(100, dbToPercent(currentDb)));
    });

    let faderDbDisplay = $derived(
        masterGain <= 0.001 
            ? '-inf' 
            : (masterGain > 1.01 ? '+' : '') + (20 * Math.log10(masterGain)).toFixed(1)
    );

    let isMenuOpen = $state(false);

    // --- MENU LOGIC ---
    function toggleMenu() { isMenuOpen = !isMenuOpen; }
    function closeMenu() { isMenuOpen = false; }

    function handleAction(action: string) {
      dispatch(action);
      closeMenu();
    }

    // --- TRANSPORT ---
    function togglePlay() { isPlaying ? dispatch('pause') : dispatch('play'); }
    function toggleRecord() { dispatch('record'); }
    function returnToStart() { dispatch('rewind'); }

    function formatTimeDisplay(totalSeconds: number) {
        const m = Math.floor(totalSeconds / 60);
        const s = Math.floor(totalSeconds % 60);
        const ms = Math.floor((totalSeconds % 1) * 10); 
        return `${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}.${ms}`;
    }

    // --- MASTER VOLUME ---
    function updateMaster(e: Event) {
        const val = parseFloat((e.target as HTMLInputElement).value);
        masterVolume = val;
        
        // Use our new taper curve!
        const targetDb = percentToDb(val);
        masterGain = targetDb <= -59.5 ? 0 : Math.pow(10, targetDb / 20);
        
        invoke('set_master_gain', { gain: masterGain });
    }

    function resetMaster() {
        masterVolume = 80;   // 80% perfectly equals 0.0 dB
        masterGain = 1.0;    // Unity Gain
        invoke('set_master_gain', { gain: 1.0 });
    }

    // --- ADDED: MASTER METER POLLING LOGIC ---
    let meterScale = $state(0);
    let rmsScale = $state(0); // <--- NEW: RMS State
    let displayDb = $state('-inf');
    let meterRunning = false;
    let reqId: number;

    function toDB(linear: number) {
        if (linear <= 0.00001) return -60;
        return Math.max(-60, 20 * Math.log10(linear));
    }

    const pollMasterMeter = async () => {
        if (!meterRunning) return;
        try {
            // Fetch Peak and RMS from the new struct
            const { peak_l, peak_r, rms_l, rms_r } = await invoke<{peak_l: number, peak_r: number, rms_l: number, rms_r: number}>('get_master_meter');
            
            const maxPeak = Math.max(peak_l, peak_r);
            const maxRms = Math.max(rms_l, rms_r);

            const pDb = toDB(maxPeak);

            // Format text: show "-inf" if silent, otherwise format to 1 decimal with optional '+' sign
            displayDb = pDb <= -59.5 ? '-inf' : (pDb > 0 ? '+' : '') + pDb.toFixed(1);

            meterScale = Math.max(0, Math.min(1.0, dbToPercent(pDb) / 100));
            rmsScale = Math.max(0, Math.min(1.0, dbToPercent(toDB(maxRms)) / 100));
        } catch(e) {}
        reqId = requestAnimationFrame(pollMasterMeter);
    };

    onMount(() => {
        meterRunning = true;
        pollMasterMeter();
    });

    onDestroy(() => {
        meterRunning = false;
        cancelAnimationFrame(reqId);
    });

  // --- NEW FUNCTIONS ---
    
    async function handleUndo() {
        try {
            await invoke('undo');
            // Optional: Trigger a state refresh if needed
             window.dispatchEvent(new CustomEvent('refresh-project'));
        } catch (e) {
            console.error("Undo failed:", e);
        }
    }

    async function handleRedo() {
        try {
            await invoke('redo');
            // Optional: Trigger a state refresh
             window.dispatchEvent(new CustomEvent('refresh-project'));
        } catch (e) {
            console.error("Redo failed:", e);
        }
    }

    // Keyboard Shortcuts
    function handleKeydown(e: KeyboardEvent) {
        if ((e.ctrlKey || e.metaKey) && e.key === 'z') {
            if (e.shiftKey) {
                handleRedo(); // Cmd+Shift+Z
            } else {
                handleUndo(); // Cmd+Z
            }
        }
        // Windows standard Redo (Ctrl+Y)
        if ((e.ctrlKey || e.metaKey) && e.key === 'y') {
            handleRedo();
        }
    }

  // --- PROJECT ACTIONS (Defined HERE, used by Menu AND Buttons) ---
</script>

<svelte:window onkeydown={handleKeydown} />

{#if isMenuOpen}
    <button 
        class="fixed inset-0 z-40 bg-transparent border-none cursor-default w-full h-full outline-none" 
        onclick={closeMenu}
        aria-label="Close Menu" 
        tabindex="-1"
    ></button>
{/if}

<div class="h-16 w-full bg-[#0f0f16] border-b border-white/10 flex items-center justify-between px-4 relative z-50">
  
    <div class="flex items-center gap-4 relative">
      
        <button onclick={toggleMenu} class={`text-white/60 hover:text-white transition-colors ${isMenuOpen ? 'text-white' : ''}`}>
            <Menu size={20} />
        </button>

        {#if isMenuOpen}
            <MenuDropdown 
                on:new={ () => handleAction('new')}
                on:load={ () => handleAction('load')}
                on:save={ () => handleAction('save')}
                on:export={ () => handleAction('export')}
            />
        {/if}

        <div class="h-6 w-px bg-white/10 mx-2"></div>
        <div class="flex items-center gap-1 bg-white/5 rounded-lg p-1">
            <button 
                onclick={handleUndo} 
                class="p-1.5 text-white/40 hover:text-white rounded transition-colors"
                title="Undo (Ctrl+Z)"
            >
                <Undo size={16} />
            </button>
        
            <button 
                onclick={handleRedo} 
                class="p-1.5 text-white/40 hover:text-white rounded transition-colors"
                title="Redo (Ctrl+Y)"
            >
                <Redo size={16} />
            </button>
        </div>
    </div>

    <div class="flex items-center gap-3 flex-1 justify-center ml-8">
        <button class="w-10 h-10 rounded-lg bg-white/5 border border-white/10 flex items-center justify-center text-white/60 hover:text-brand-blue hover:border-brand-blue/50 transition-all">
            <Timer size={18} />
        </button>

        <div class="flex items-center bg-white/5 border border-white/10 rounded-lg h-10 px-3 gap-2">
            <input type="number" bind:value={bpm} class="bg-transparent w-12 text-center font-mono text-sm focus:outline-none" />
            <span class="text-xs text-white/40">bpm</span>
            <div class="h-4 w-px bg-white/10"></div>
            <input type="text" bind:value={timeSignature} class="bg-transparent w-12 text-center font-mono text-sm focus:outline-none" />
        </div>
    </div>

    <div class="flex items-center gap-6 flex-1 justify-center">
        <div class="flex items-center gap-2">
            <button onclick={returnToStart} class="w-10 h-10 rounded-full flex items-center justify-center bg-white/5 hover:bg-white/10 text-white/60 hover:text-white transition-all active:scale-95" title="Return to Start">
                <SkipBack size={16} class="fill-current" />
            </button>

            <button onclick={togglePlay} class={`w-10 h-10 rounded-full flex items-center justify-center transition-all ${isPlaying ? 'bg-brand-blue text-white shadow-lg shadow-brand-blue/50' : 'bg-white/5 hover:bg-white/10 text-white'}`}>
                {#if isPlaying} 
                  <Pause size={16} class="fill-current" /> 
                {:else} 
                  <Play size={16} class="fill-current ml-0.5" /> 
                {/if}
            </button>

            <button onclick={toggleRecord} class={`w-10 h-10 rounded-full flex items-center justify-center transition-all ${isRecording ? 'bg-brand-red text-white shadow-lg shadow-brand-red/50 animate-pulse' : 'bg-white/5 hover:bg-white/10 text-brand-red'}`}>
                {#if isRecording}
                    <Square size={14} class="fill-current" />
                {:else}    
                    <Circle size={14} class="fill-current" />
                {/if}    
            </button>
        </div>

        <div class="bg-black/30 border border-white/10 rounded-lg px-4 py-2 font-mono text-xl tracking-wider text-white/90 w-32 text-center">
            {formatTimeDisplay(currentTime)}
        </div>
    </div>

    <div class="flex items-center gap-4 justify-end flex-1">
      
        <button class="h-9 px-3 rounded-lg bg-brand-purple/10 border border-brand-purple/30 flex items-center gap-2 text-sm text-brand-purple hover:bg-brand-purple/20 transition-all">
            <SlidersHorizontal size={14} /> Mastering
        </button>

        <div class="flex items-center gap-2 mx-2 group">
            <Volume2 size={16} class="text-white/40 group-hover:text-white transition-colors" />
            
            <div class="relative w-26 h-2.5 flex items-center">
 
                <div class="absolute inset-0 w-full h-full bg-[#0f0f16] rounded-full overflow-hidden border border-white/10 shadow-inner">
                    <div class="absolute inset-0 w-full h-full" 
                         style="background: linear-gradient(to right, #4ade80 0%, #4ade80 56%, #eab308 56%, #eab308 72%, #ef4444 72%, #ef4444 100%);">
                    </div>
                 
                    <div class="absolute right-0 top-0 bottom-0 bg-[#0f0f16]" 
                         style="width: {(1 - meterScale) * 100}%; transition: width 0.05s linear;">
                    </div>

                    <div class="absolute left-0 top-0 bottom-0 bg-white/40 border-r border-white/60" 
                         style="width: {rmsScale * 100}%; transition: width 0.05s linear;">
                    </div>
                    
                    <div class="absolute right-0 top-0 bottom-0 bg-black/50 backdrop-blur-sm z-10 border-l border-white/20" 
                         style="width: {100 - masterVolume}%;">
                    </div>
                </div>

                <input 
                    type="range" min="0" max="100" 
                    value={masterVolume} 
                    oninput={updateMaster} 
                    ondblclick={resetMaster}
                    class="absolute inset-0 w-full h-full z-20 appearance-none bg-transparent cursor-pointer outline-none
                           [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:w-4 [&::-webkit-slider-thumb]:h-4 
                           [&::-webkit-slider-thumb]:bg-transparent [&::-webkit-slider-thumb]:border-2 [&::-webkit-slider-thumb]:border-white
                           [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:shadow-[0_0_5px_rgba(0,0,0,0.8)]
                           hover:[&::-webkit-slider-thumb]:scale-110 [&::-webkit-slider-thumb]:transition-transform"
                    title="Master Volume"
                />
            </div>
            <div class="w-8 text-right font-mono text-[10px] text-white/50 group-hover:text-white/90 transition-colors pointer-events-none tracking-tighter">
                {faderDbDisplay}
            </div>
        </div>

        <div class="h-6 w-px bg-white/10 mx-2"></div>

        <button 
            onclick={() => dispatch('save')}
            class="h-9 px-3 rounded-lg bg-white/5 border border-white/10 flex items-center gap-2 text-sm text-white/70 hover:bg-white/10 hover:text-white transition-all"
        >
            <Save size={16} /> Save
        </button>

        <button 
            onclick={() => dispatch('export')}
            class="h-9 px-4 rounded-lg bg-brand-blue flex items-center gap-2 text-sm font-medium text-white hover:bg-brand-blue/80 transition-all shadow-lg shadow-brand-blue/20"
        >
            <Share size={16} /> Export
        </button>

    </div>
</div>