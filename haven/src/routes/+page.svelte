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
    import { recordingManager } from '$lib/managers/RecordingManager';

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

    let isRecordingMode = $state(false);

    type Clip = {
        id: string;         // Unique ID for the clip (e.g., "clip_1_123")
        trackId: number;    // Which track this belongs to
        name: string;
        path: string;       // File path (Source of truth)
        startTime: number;  // Where it sits on the timeline
        duration: number;
        offset: number;     // (For trimming later)
        waveform: { mins: number[], maxs: number[], duration: number, binsPerSecond: number };
        color: string;
    };
    // --- TYPE DEFINITION (UPDATED) ---
    type Track = {
      id: number;
      name: string;
      color: string;
      clips: Clip[];
      // duration?: number;
      // startTime?: number; 
      // waveform?: { mins: number[], maxs: number[] };
      // NEW FIELDS FOR MIXER:
      gain: number;
      pan: number;
      muted: boolean;
      solo: boolean;
      isRecording?: boolean;
      savePath?: string;
      source: 'mic' | 'media';
      monitor: boolean;
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
        const maxId = tracks.length > 0 ? Math.max(...tracks.map(t => t.id)) : 0;
        const id = maxId + 1;
        
        const colors = [
            'bg-brand-blue', 
            'bg-brand-red', 
            'bg-purple-500', 
            'bg-emerald-500', 
            'bg-orange-500', 
            'bg-pink-500',
            'bg-cyan-500',   // Added extra variety
            'bg-indigo-500', // Added extra variety
            'bg-rose-500'    // Added extra variety
        ];
        const color = colors[Math.floor(Math.random() * colors.length)];
        
        // Default mixer values
        const defaultMixer = {
            gain: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
            monitor: false
        };
      
        if (type === 'record') {
            // 1. Setup File Path (In real app, use a project folder)
            tracks = tracks.map(t => ({ ...t, isRecording: false }));
            const filename = `Recording_${id}.wav`;
            const savePath = await invoke<string>('get_temp_path', { filename }); 

            // 2. Add "Placeholder" Track (Red, growing)
            tracks = [...tracks, { 
                id, 
                name: "Recording...", 
                color: color, 
                // startTime: currentTime, 
                // duration: 0, 
                clips: [],
                ...defaultMixer,
                isRecording: true, // Flag for custom styling if needed
                savePath: savePath,
                source: 'mic'
            }];
            try {
                await invoke('create_track');
                console.log("✅ Backend track created.");
            } catch (e) {
                console.error("Failed to create backend track:", e);
            }
            // ----------------------------------------
          
            console.log("Track Armed. Press Record button to start.");
          
        }
        else if (type === 'import' || type === 'upload') {
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
                        binsPerSecond: number, 
                        bpm?: number 
                    }>('import_track', { path: selected });

                    const filename = selected.split(/[\\/]/).pop() || `Imported ${id}`;
                  
                    if (result.bpm && result.bpm > 0) {
                        console.log("Detected BPM:", result.bpm);
                        bpm = Math.round(result.bpm); 
                    }
                  
                    const newClip: Clip = {
                        id: `clip_${Date.now()}`,
                        trackId: id,
                        name: filename,
                        path: selected,
                        startTime: 0,
                        duration: result.duration,
                        offset: 0,
                        waveform: { mins: result.mins, maxs: result.maxs, duration: result.duration, binsPerSecond: result.binsPerSecond },
                        color: color
                    };

                    tracks = [...tracks, { 
                        id, 
                        name: filename, 
                        color, 
                        clips: [newClip],
                        ...defaultMixer, // Spread mixer defaults
                        isRecording: false,
                        source: 'media'
                    }];
                }
            } catch (e) {
                console.error("Import failed:", e);
            } finally {
                isLoading = false;
            }
        } 
        else {
            const name = `Track ${id}`;
            tracks = [...tracks, { id, name, color, clips: [],  ...defaultMixer, isRecording: false, source: 'media' }];
        }
    }

    async function handleDeleteTrack(event: CustomEvent<number>) {
        const index = event.detail;
        
        // 1. Optimistic Update
        tracks.splice(index, 1);
        tracks = [...tracks]; // Trigger reactivity

        // 2. Call Backend
        try {
            await invoke('delete_track', { trackIndex: index });
            console.log("🗑️ Track deleted");
        } catch (e) {
            console.error("Failed to delete track:", e);
            alert("Error deleting track: " + e);
            refreshProjectState(); // Rollback/Sync on error
        }
    }

    // --- NEW HELPER: Exclusive Arming ---
    function armTrack(trackId: number) {
        // Set isRecording = true ONLY for the matching ID, false for everyone else
        tracks = tracks.map(t => ({
            ...t,
            isRecording: t.id === trackId
        }));
        console.log(`🎙️ Track ${trackId} Armed`);
    }

    async function startRecordingLogic() {
        // Find the armed track
        const trackIndex = tracks.findIndex(t => t.isRecording === true);
        if (trackIndex === -1) { alert("No track armed!"); return; }

        const trackId = tracks[trackIndex].id; // Get the real ID (e.g., 1, 2, 3)
        const trackColor = tracks[trackIndex].color;

        try {
            // 1. Generate Path
            const timestamp = Date.now();
            const filename = `Recording_${trackId}_${timestamp}.wav`;
            const savePath = await invoke<string>('get_temp_path', { filename });

            isRecordingMode = true;
            
            // 2. Create a "Ghost" Clip for visualization
            const newClip: Clip = {
                id: `clip_${timestamp}`,
                trackId: trackId,
                name: "Recording...",
                path: savePath,
                startTime: currentTime, // Start at playhead
                duration: 0,
                offset: 0,
                waveform: { mins: [], maxs: [], duration: 0, binsPerSecond: 100 }, 
                color: trackColor
            };

            // 3. Push to Track
            tracks[trackIndex].clips.push(newClip);
            tracks[trackIndex].savePath = savePath;
            tracks = tracks; // Reactivity

            // 4. Start Engine
            if (!isPlaying) {
               await invoke('play');
               isPlaying = true;
               pollPosition();
            }

            // 5. Start Manager (FIXED ARGUMENTS)
            // We pass: path, trackId, startTime, callback
            await recordingManager.start(
                savePath, 
                trackId, 
                currentTime, 
                (newDuration) => {
                    // Update the *Last* clip in the list
                    const tIdx = tracks.findIndex(t => t.id === trackId);
                    if (tIdx !== -1) {
                        const clips = tracks[tIdx].clips;
                        if (clips.length > 0) {
                            clips[clips.length - 1].duration = newDuration;
                            tracks = tracks; 
                        }
                    }
                }
            );

            if (tracks[trackIndex].monitor) {
                try {
                    // The recorder defaults to monitor OFF. We toggle it ON here if needed.
                    await invoke('toggle_monitor_cmd');
                    console.log("🔊 Monitor Enabled for recording");
                } catch (e) {
                    console.error("Failed to enable monitor:", e);
                }
            }

        } catch (e) {
            console.error("Failed to start:", e);
            isRecordingMode = false;
        }
    }

    async function handleToggleMonitor(event: CustomEvent<number>) {
        const trackId = event.detail;
        const tIdx = tracks.findIndex(t => t.id === trackId);
        if (tIdx === -1) return;

        // 1. Toggle UI State
        tracks[tIdx].monitor = !tracks[tIdx].monitor;
        console.log(`Track ${trackId} Monitor: ${tracks[tIdx].monitor ? 'ON' : 'OFF'}`);

        // 2. If we are currently Recording AND this is the active track, toggle Backend
        if (isRecordingMode && tracks[tIdx].isRecording) {
            try {
                await invoke('toggle_monitor_cmd');
            } catch (e) {
                console.error("Failed to toggle monitor:", e);
            }
        }
    }

    // --- 4. UPDATED STOP LOGIC ---
    async function stopRecordingLogic() {
        await invoke('pause');
        isPlaying = false;
        cancelAnimationFrame(animationFrameId);

        isRecordingMode = false;
        const result = await recordingManager.stop();

        if (result) {
            const tIdx = tracks.findIndex(t => t.isRecording);
            const trackColor = tracks[tIdx].color;

            if (tIdx !== -1) {
                const clips = tracks[tIdx].clips;
                const lastClipIdx = clips.length - 1;

                if (lastClipIdx >= 0) {
                    // Finalize the clip data
                    // We update the specific clip with the new Waveform data
                    tracks[tIdx].clips[lastClipIdx] = {
                        ...tracks[tIdx].clips[lastClipIdx],
                        name: `Take ${clips.length}`,
                        color: trackColor,
                        duration: result.duration,
                        waveform: { mins: result.mins, maxs: result.maxs, duration: result.duration, binsPerSecond: (result as any).binsPerSecond ?? 100 } 
                    };

                    // Cleanup recording state
                    tracks[tIdx].savePath = undefined;

                    // CRITICAL: Trigger Svelte Reactivity
                    tracks = [...tracks]; 

                    console.log("🌊 Visuals updated for track", tracks[tIdx].id);
                }
            }
        }
    }

    // NEW: Function to refresh tracks from backend
    async function refreshProjectState() {
        try {
            isLoading = true;
            loadingMessage = "Updating Project...";

            const projectState = await invoke<{
              tracks: Track[],
              bpm: number,
              masterGain: number
            }>('get_project_state');

            // Force reactivity update
            tracks = projectState.tracks;
            bpm = projectState.bpm;

            console.log("🔄 Project State Refreshed");
        } catch (e) {
            console.error("Failed to refresh project:", e);
        } finally {
            isLoading = false;
        }
    }

    // --- PLAYBACK LOOP ---
    async function togglePlayback() {
        if (isRecordingMode) { await stopRecordingLogic(); return; }

        // Get max duration of project
        let maxDur = 0;
        tracks.forEach(t => {
            t.clips.forEach(c => {
                if (c.startTime + c.duration > maxDur) maxDur = c.startTime + c.duration;
            });
        });

        if (isPlaying) {
            await invoke('pause');
            isPlaying = false;
            cancelAnimationFrame(animationFrameId);
        } else {
            // AUTO-REWIND FIX
            // If we are within 0.1s of the end (or past it), restart from 0
            if (currentTime >= maxDur - 0.1 && maxDur > 0) {
                await seekTo(0);
            }

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

    // --- NEW: Handle Track Selection (Arming) ---
    function handleTrackSelect(event: CustomEvent<number>) {
        const selectedId = event.detail;

        // Update tracks state: 
        // Set isRecording = true for the clicked track
        // Set isRecording = false for ALL other tracks
        tracks = tracks.map(t => ({
            ...t,
            isRecording: t.id === selectedId
        }));

        console.log(`🎙️ Track ${selectedId} Armed for Recording`);
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

<svelte:window on:keydown={handleKeydown} />

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
        isRecording={isRecordingMode} 
        on:play={togglePlayback} 
        on:pause={togglePlayback}
        on:rewind={rewind}
        on:record={() => {
            if (isRecordingMode) {
                stopRecordingLogic();
            } else {
                startRecordingLogic();
            }    
        }}
        on:record-add={() => addNewTrack('record')}
        on:new={() => window.location.reload()}
        on:load={handleLoad}
        on:save={handleSave}
        on:export={handleExport} 
    />

    <div class="flex-1 flex overflow-hidden relative">
        <TrackList {tracks} 
            on:requestAdd={handleAddRequest}
            on:select={handleTrackSelect}
            on:toggleMonitor={handleToggleMonitor}
            on:delete={handleDeleteTrack}
        />
        
        <Timeline 
            bind:tracks={tracks} 
            currentTime={currentTime} 
            bpm={bpm} 
            on:seek={(e) => seekTo(e.detail)}
            on:select={handleTrackSelect}
            on:refresh={refreshProjectState}
        /> 

    </div>

  {/if}

</main>