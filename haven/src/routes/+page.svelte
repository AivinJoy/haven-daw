<!-- haven\src\routes\+page.svelte -->
<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { open as openDialog, save } from '@tauri-apps/plugin-dialog';
  import { onDestroy } from 'svelte';
  import { listen } from '@tauri-apps/api/event';

  import LandingModal from '$lib/components/LandingModal.svelte';
  import TrackList from '$lib/components/TrackList.svelte';
  import Header from '$lib/components/Header.svelte';
  import TopToolbar from '$lib/components/TopToolbar.svelte';
  import Timeline from '$lib/components/Timeline.svelte';
  import Loader from '$lib/components/Loader.svelte';

  // --- STATE ---
  let view: 'landing' | 'studio' = $state('landing');
  let showModal = $state(false); 
  let isPlaying = $state(false);
  let currentTime = $state(0); 
  
  // --- GLOBAL BPM STATE ---
  let bpm = $state(120); 
  let projectName = $state("untitled Project");

  let loadProgress = $state(0);
  listen('load-progress', (e) => loadingMessage = e.payload as string);
  listen('load-percent', (e) => loadProgress = e.payload as number);

  let isLoading = $state(false);
  let loadingMessage = $state("Processing...");
  listen('load-progress', (event) => {
    loadingMessage = event.payload as string;
  });

  // --- TYPE DEFINITION (UPDATED) ---
  type Track = {
    id: number;
    name: string;
    color: string;
    duration?: number;
    startTime?: number; 
    waveform?: { mins: number[], maxs: number[] };
    // NEW FIELDS FOR MIXER:
    gain: number;
    pan: number;
    muted: boolean;
    solo: boolean;
  };

  // --- TRACKS STATE ---
  let tracks = $state<Track[]>([]);
  let animationFrameId: number;

  // Sync BPM with Backend whenever it changes
  $effect(() => {
      if (bpm > 0) {
          invoke('set_bpm', { bpm: bpm }).catch(e => console.error("Failed to set BPM:", e));
      }
  });

  async function handleLoad() {
      try {
          const path = await openDialog({
              filters: [{ name: 'Haven Project', extensions: ['hvn'] }]
          });
          
          if (path && typeof path === 'string') {
              isLoading = true;
              loadingMessage = "initializing...";
              
              // 1. Backend: Load Engine State & Re-Analyze Waveforms
              const projectState = await invoke<{
                tracks: Track[],
                bpm: number,
                masterGain: number
              }>('load_project', { path });
              
              bpm = projectState.bpm;

              const fileName = path.split(/[\\/]/).pop();
              if (fileName) projectName = fileName.replace('.hvn', '');
              
              // 2. Frontend: Update State
              tracks = projectState.tracks;
              
              // 3. Update View
              view = 'studio';
              showModal = false;
              
              console.log("✅ Project Loaded:", tracks.length, "tracks");
          }
      } catch (e) {
          console.error("Load failed:", e);
          alert("Failed to load: " + e);
      } finally {
          isLoading = false;
      }
  }

  async function handleSave() {
      try {
          let filename = projectName.trim() || "Untitled Project";
          if (!filename.endsWith('.hvn')) filename += '.hvn';
          const path = await save({
                defaultPath: filename,  
                filters: [{ name: 'Haven Project', extensions: ['hvn'] }]
          });
          
          if (path) {
              isLoading = true;
              loadingMessage = "Saving Project...";
              await invoke('save_project', { path });
              const newName = path.split(/[\\/]/).pop();
              if (newName) projectName = newName.replace('.hvn', '');
          }
      } catch (e) {
          console.error("Save failed:", e);
      } finally {
          isLoading = false;
      }
  }

  async function handleExport() {
        try {
                let filename = projectName.trim() || "Untitled Project";
                if (!filename.endsWith('.wav')) filename += '.wav';
                const path = await save({
                    defaultPath: filename,
                    filters: [{ name: 'WAV Audio', extensions: ['wav'] }]
                });
            
                if (path) {
                    if (isPlaying) { await invoke('pause'); isPlaying = false; }

                    isLoading = true;
                    loadingMessage = "Rendering Audio...";
                    if (isPlaying) {
                      await invoke('pause');
                      isPlaying = false;
                    }
                    await invoke('export_project', { path });
                    alert("Export Complete!");
                }
            } catch (e) {
                console.error("Export failed:", e);
            } finally {
                isLoading = false;
            }
  }

  // --- HANDLERS ---
  async function handleInitialSelection(event: CustomEvent<string>) {
    await addNewTrack(event.detail);
    view = 'studio';
  }

  function handleAddRequest() { showModal = true; }

  async function handleModalSelection(event: CustomEvent<string>) {
    await addNewTrack(event.detail);
    showModal = false;
  }

  // --- CORE LOGIC: ADD TRACK + BACKEND IMPORT ---
  async function addNewTrack(type: string) {
    const id = tracks.length + 1;
    
    const colors = [
        'bg-brand-blue', 'bg-brand-red', 'bg-purple-500', 
        'bg-emerald-500', 'bg-orange-500', 'bg-pink-500'
    ];
    const color = colors[(id - 1) % colors.length];

    // Default mixer values
    const defaultMixer = {
        gain: 1.0,
        pan: 0.0,
        muted: false,
        solo: false
    };

    if (type === 'import' || type === 'upload') {
        try {
            const selected = await openDialog({
                multiple: false,
                filters: [{ name: 'Audio', extensions: ['wav', 'mp3', 'flac', 'ogg'] }]
            });

            if (selected && typeof selected === 'string') {

                isLoading = true;
                loadingMessage = "Importing Track..."

                const result = await invoke<{
                    mins: number[], 
                    maxs: number[], 
                    duration: number, 
                    bpm?: number 
                }>('import_track', { path: selected });
                
                const filename = selected.split(/[\\/]/).pop() || `Imported ${id}`;

                if (result.bpm && result.bpm > 0) {
                    console.log("Detected BPM:", result.bpm);
                    bpm = Math.round(result.bpm); 
                }
                
                tracks = [...tracks, { 
                    id, 
                    name: filename, 
                    color, 
                    duration: result.duration, 
                    startTime: 0, 
                    waveform: { mins: result.mins, maxs: result.maxs },
                    ...defaultMixer // Spread mixer defaults
                }];
            }
        } catch (e) {
            console.error("Import failed:", e);
        } finally {
            isLoading = false;
        }
    } 
    else if (type === 'record') {
        const name = `Recording ${id}`;
        tracks = [...tracks, { id, name, color, startTime: 0, ...defaultMixer }];
    } 
    else {
        const name = `Track ${id}`;
        tracks = [...tracks, { id, name, color, startTime: 0, ...defaultMixer }];
    }
  }

  // --- PLAYBACK LOOP ---
  async function togglePlayback() {
      if (isPlaying) {
          await invoke('pause');
          isPlaying = false;
          cancelAnimationFrame(animationFrameId);
      } else {
          await invoke('play');
          isPlaying = true;
          pollPosition();
      }
  }

  function pollPosition() {
      invoke<number>('get_position')
          .then((pos) => {
              currentTime = pos;
              if (isPlaying) {
                  animationFrameId = requestAnimationFrame(pollPosition);
              }
          })
          .catch(console.error);
  }

  async function seekTo(time: number) {
      currentTime = time; 
      try {
          await invoke('seek', { pos: time });
      } catch (e) {
          console.error("Seek failed:", e);
      }
  }

  async function rewind() {
      await seekTo(0); 
      if (isPlaying) {
          await invoke('pause');
          isPlaying = false;
          cancelAnimationFrame(animationFrameId);
      }
  }


  function handleKeydown(e: KeyboardEvent) {
      if (view !== 'studio') return;

      switch (e.code) {
          case 'Space':
              e.preventDefault();
              togglePlayback();
              break;
          
          case 'ArrowLeft':
              e.preventDefault();
              seekTo(Math.max(0, currentTime - (e.shiftKey ? 10 : 5)));
              break;

          case 'ArrowRight':
              e.preventDefault();
              seekTo(currentTime + (e.shiftKey ? 10 : 5));
              break;
      }
  }

  onDestroy(() => {
    cancelAnimationFrame(animationFrameId);
  });
</script>

<svelte:window onkeydown={handleKeydown} />

{#if isLoading}
    <Loader message={loadingMessage} progress={loadProgress} />
{/if}    

<main class="h-screen w-screen bg-[#0f0f16] text-white overflow-hidden relative font-sans flex flex-col">
  
  {#if view === 'landing' || showModal}
    <div class="absolute inset-0 z-50">
        <LandingModal on:select={view === 'landing' ? handleInitialSelection : handleModalSelection} />
    </div>
  {/if}

  {#if view === 'studio'}
    <Header bind:projectName={projectName}/>
    
    <TopToolbar 
        isPlaying={isPlaying} 
        currentTime={currentTime}
        bind:bpm={bpm} 
        on:play={togglePlayback} 
        on:pause={togglePlayback}
        on:rewind={rewind}
        on:record={() => addNewTrack('record')}
        
        on:new={() => window.location.reload()}
        on:load={handleLoad}
        on:save={handleSave}
        on:export={handleExport} 
    />

    <div class="flex-1 flex overflow-hidden relative">
        <TrackList {tracks} on:requestAdd={handleAddRequest} />
        
        <Timeline {tracks} currentTime={currentTime} bpm={bpm} 
        on:seek={(e) => seekTo(e.detail)}/> 

    </div>

  {/if}

</main>