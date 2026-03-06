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
    import EqWindow from "$lib/components/EqWindow.svelte";
    import CompressorWindow from "$lib/components/CompressorWindow.svelte";
    import AIChatbot from '$lib/components/AIChatbot.svelte';

    // --- STATE ---
    let view: 'landing' | 'studio' = $state('landing');
    let showModal = $state(false); 
    let isPlaying = $state(false);
    let currentTime = $state(0); 
    let showEqWindow = $state(false);
    let eqTrackId = $state(0);
    let showCompressorWindow = $state(false); // <--- ADD THIS
    let compressorTrackId = $state(0);
    
    // --- GLOBAL BPM STATE ---
    let bpm = $state(120); 
    let timeSignatureNumerator = $state(4);
    let masterGain = $state(1.0); // Add this near 'bpm'

    // Sync BPM & Time Signature with Backend whenever they change
    $effect(() => {
        if (bpm > 0) {
            invoke('set_bpm', { bpm: bpm }).catch(e => console.error("Failed to set BPM:", e));
        }
        if (timeSignatureNumerator > 0) {
            invoke('set_time_signature', { numerator: timeSignatureNumerator })
                .catch(e => console.error("Failed to set Time Sig:", e));
        }
    });

    let projectName = $state("untitled Project");

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
    // --- PLAYHEAD TIMING ARCHITECTURE ---
    let animationFrameId: number | null = null;
    let syncIntervalId: ReturnType<typeof setInterval> | null = null;
    let lastSyncTime = 0;
    let lastEnginePosition = 0;

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
                // 1. Backend: Load Engine State & Re-Analyze Waveforms
                const projectState = await invoke<{
                  tracks: Track[],
                  bpm: number,
                  timeSignatureNumerator?: number, // Made optional to not break old save files
                  masterGain: number
                }>('load_project', { path }); // (Do the exact same update in 'get_project_state')

                bpm = projectState.bpm;
                if (projectState.timeSignatureNumerator) {
                    timeSignatureNumerator = projectState.timeSignatureNumerator;
                }
                

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
                await invoke('save_project', { path });
                const newName = path.split(/[\\/]/).pop();
                if (newName) projectName = newName.replace('.hvn', '');
            }
        } catch (e) {
            console.error("Save failed:", e);
        } finally {
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
    // --- CORE LOGIC: ADD TRACK (STRICT BACKEND AUTHORITY) ---
    async function addNewTrack(type: string) {
        
        // 1. We only define the UI flags here. 
        // Gain, Pan, Mute, Solo come from the Backend in 'newTrack'.
        const defaultMixerUI = {
            isRecording: false,
            source: 'media' as const,
            monitor: false
        };

        if (type === 'record') {
            try {
                // Ask Backend to Create Track (Returns ID, Name, Color, Gain, Pan, etc.)
                const newTrack = await invoke<Track>('create_track');
                
                // Setup Recording Path
                const filename = `Recording_${newTrack.id}.wav`;
                const savePath = await invoke<string>('get_temp_path', { filename });

                // Merge Backend Data + UI Flags
                tracks = [...tracks, { 
                    ...newTrack,          // <--- This brings in gain:1.0, pan:0.0, etc.
                    ...defaultMixerUI,    // <--- This adds monitor:false, source:'media'
                    // name: "Recording...", 
                    isRecording: true,    
                    source: 'mic',
                    savePath: savePath
                }];
                
                console.log(`✅ Track ${newTrack.id} Created & Armed.`);
            } catch (e) {
                console.error("Failed to create backend track:", e);
            }
        }
        else if (type === 'import' || type === 'upload') {
            try {
                const selected = await openDialog({
                    multiple: true,
                    filters: [{ name: 'Audio', extensions: ['wav', 'mp3', 'flac', 'ogg'] }]
                });
                
                if (selected) {
                    const paths = Array.isArray(selected) ? selected : [selected];

                    // Backend handles import and returns analysis
                    const results = await invoke<any[]>('import_tracks', { paths });

                    if (tracks.length === 0 && results.length > 0 && results[0].bpm) {
                        bpm = Math.round(results[0].bpm);
                    }

                    // Refresh to get the tracks created by the import command
                    await refreshProjectState(); 
                }
            } catch (e) {
                console.error("Import failed:", e);
            }
        } 
        else {
             // Generic Empty Track
             try {
                const newTrack = await invoke<Track>('create_track');
                // Merge Backend Data + UI Flags
                tracks = [...tracks, { 
                    ...newTrack, 
                    ...defaultMixerUI 
                }];
             } catch(e) {
                 console.error("Failed to add track", e);
             }
        }
    }

    async function handleDeleteTrack(event: CustomEvent<number>) {
        const trackId = event.detail;
        
        // 1. Call Backend
        try {
            await invoke('delete_track', { trackId: trackId });
            console.log("🗑️ Track deleted");
            // 2. Sync State (Safest way to ensure IDs stay aligned)
            await refreshProjectState();
        } catch (e) {
            console.error("Failed to delete track:", e);
            alert("Error deleting track: " + e);
        }
    }

    function handleOpenEq(event: CustomEvent<number>) {
        eqTrackId = event.detail; // TrackList sends the index (0, 1, 2...)
        showEqWindow = true;
    }

    function handleOpenCompressor(event: CustomEvent<number>) {
        console.log("🔥 OPEN COMPRESSOR CLICKED FOR TRACK:", event.detail);
        compressorTrackId = event.detail;
        showCompressorWindow = true;
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
               // Fetch initial hardware position to anchor the interpolation perfectly
               lastEnginePosition = await invoke<number>('get_position');
               currentTime = lastEnginePosition;
               startPlaybackLoop();
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
   // --- STOP LOGIC ---
    async function stopRecordingLogic() {
        await invoke('pause');
        isPlaying = false;
        stopPlaybackLoop(); // Safely clears rAF and interval

        isRecordingMode = false;
        const result = await recordingManager.stop();
        
        if (result) {
            // After recording stops, the backend has analyzed and cached the file.
            // We refresh the project state to get the final, authoritative clip data
            // instead of trying to patch it manually.
            await refreshProjectState();
            console.log("🌊 Project Synced after recording");
        }
    }

    // NEW: Function to refresh tracks from backend
    // Refresh tracks from backend
    async function refreshProjectState() {
        try {
            const projectState = await invoke<{
              tracks: Track[],
              bpm: number,
              masterGain: number
            }>('get_project_state');
            
            // We preserve 'isRecording', 'monitor', and 'source' flags which are UI-only
            // by merging them back into the fresh state based on ID matching.
            const oldStateMap = new Map(tracks.map(t => [t.id, t]));
            
            tracks = projectState.tracks.map(newTrack => {
                const oldTrack = oldStateMap.get(newTrack.id);
                return {
                    ...newTrack,
                    isRecording: oldTrack ? oldTrack.isRecording : false,
                    monitor: oldTrack ? oldTrack.monitor : false,
                    source: oldTrack ? oldTrack.source : newTrack.source,
                    savePath: oldTrack ? oldTrack.savePath : undefined
                };
            });

            bpm = projectState.bpm;
            masterGain = projectState.masterGain;
            console.log("🔄 Project State Refreshed");
        } catch (e) {
            console.error("Failed to refresh project:", e);
        } 
    }

    // --- NEW: Listen for Undo/Redo Events ---
    // --- NEW: Listen for Undo/Redo Events ---
    // --- NEW: Global Event Listeners (Undo/Redo + AI Commands) ---
    $effect(() => {
        const handleRefresh = () => refreshProjectState();

        // --- NEW: ASYNC COMMAND QUEUE ---
        // Prevents Race Conditions when AI sends multiple commands at once
        let aiCommandQueue: any[] = [];
        let isProcessingQueue = false;

        const processQueue = async () => {
            if (isProcessingQueue) return;
            isProcessingQueue = true;

            while (aiCommandQueue.length > 0) {
                const detail = aiCommandQueue.shift();
                const { action, mode, time, direction } = detail;
                console.log("⚡ Processing AI Command:", action, mode);

                switch (action) {
                    case 'play':
                        if (!isPlaying) togglePlayback();
                        break;
                    case 'pause':
                        if (isPlaying) togglePlayback();
                        break;
                    case 'rewind':
                        rewind(); 
                        break;

                    case 'seek':
                        if (time === undefined) break;
                        let targetTime = time;
                        if (direction === 'forward') {
                            targetTime = currentTime + time;
                        } else if (direction === 'backward') {
                            targetTime = currentTime - time;
                        }
                        seekTo(Math.max(0, targetTime));
                        break;

                    case 'record':
                        if (detail.trackId !== undefined) {
                            console.log("🤖 AI Arming Track:", detail.trackId);
                            armTrack(detail.trackId);
                        } else if (!isRecordingMode && tracks.length > 0 && !tracks.some(t => t.isRecording)) {
                            console.log("🤖 Auto-Arming newest track");
                            armTrack(tracks[tracks.length - 1].id);
                        }

                        if (isRecordingMode) stopRecordingLogic();
                        else startRecordingLogic(); 
                        break;

                    case 'create_track':
                        // AWAIT is crucial here so the queue pauses until Rust finishes making the track
                        if (mode === 'record') await addNewTrack('record');
                        else await addNewTrack('default');
                        
                        // Give Svelte a tiny moment to update the DOM/tracks array before moving to the next command
                        await new Promise(resolve => setTimeout(resolve, 50));
                        break;

                    case 'toggle_monitor':
                        let targetTrackId = detail.trackId;
                        
                        // Fallbacks if AI didn't specify a track ID
                        if (targetTrackId === undefined) {
                            const armed = tracks.find(t => t.isRecording);
                            if (armed) {
                                targetTrackId = armed.id; // Target armed track
                            } else if (tracks.length > 0) {
                                targetTrackId = tracks[tracks.length - 1].id; // Target newest track
                            }
                        }

                        if (targetTrackId !== undefined) {
                            handleToggleMonitor(new CustomEvent('toggle', { detail: targetTrackId }));
                        }
                        break;
                }
            }
            isProcessingQueue = false;
        };

        const handleAICommand = (e: Event) => {
            const customEvent = e as CustomEvent;
            // Push the command into the queue instead of running it immediately
            aiCommandQueue.push(customEvent.detail);
            processQueue(); // Kick off the queue processor
        };
        
        window.addEventListener('refresh-project', handleRefresh);
        window.addEventListener('ai-command', handleAICommand);
        
        return () => {
            window.removeEventListener('refresh-project', handleRefresh);
            window.removeEventListener('ai-command', handleAICommand);
        };
    });

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
            stopPlaybackLoop(); // Safely clears rAF and interval
        } else {
            // AUTO-REWIND FIX
            // If we are within 0.1s of the end (or past it), restart from 0
            if (currentTime >= maxDur - 0.1 && maxDur > 0) {
                await seekTo(0);
            }

            await invoke('play');
            isPlaying = true;
            
            // Fetch initial hardware position to anchor the interpolation perfectly
            lastEnginePosition = await invoke<number>('get_position');
            currentTime = lastEnginePosition;
            startPlaybackLoop();
        }
    }

    // 1. Low-Frequency Hardware Sync (Corrects JS drift against Rust clock)
    async function syncEngineClock() {
        if (!isPlaying) return;
        try {
            const pos = await invoke<number>('get_position');
            lastEnginePosition = pos;
            lastSyncTime = performance.now();
            
            // Hard snap if background tab throttling caused severe drift (> 50ms)
            if (Math.abs(currentTime - pos) > 0.05) {
                currentTime = pos;
            }
        } catch (e) {
            console.error("Hardware sync failed:", e);
        }
    }

    // 2. High-Frequency UI Interpolation (Runs at Monitor Refresh Rate)
    function startPlaybackLoop() {
        // GUARD: Prevent multiple ghost loops
        if (!isPlaying || animationFrameId) return;
        
        lastSyncTime = performance.now();
        
        const renderLoop = (now: number) => {
            if (!isPlaying) return;
            
            const deltaSec = (now - lastSyncTime) / 1000.0;
            currentTime = lastEnginePosition + deltaSec; // Optimistic smooth advance
            
            animationFrameId = requestAnimationFrame(renderLoop);
        };
        
        animationFrameId = requestAnimationFrame(renderLoop);
        syncIntervalId = setInterval(syncEngineClock, 100);
    }

    // 3. Centralized Cleanup Helper
    function stopPlaybackLoop() {
        if (animationFrameId) {
            cancelAnimationFrame(animationFrameId);
            animationFrameId = null;
        }
        if (syncIntervalId) {
            clearInterval(syncIntervalId);
            syncIntervalId = null;
        }
    }

    async function seekTo(time: number) {
        currentTime = time;
        
        // Update anchor points so the interpolator doesn't fight the seek
        lastEnginePosition = time; 
        lastSyncTime = performance.now(); 
        
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
            stopPlaybackLoop(); // Safely clears both rAF and the interval
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

        // 🛑 FIX: Ignore global shortcuts if user is typing in an Input or Textarea
        const target = e.target as HTMLElement;
        if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA') return;

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
      stopPlaybackLoop(); // Safely clears rAF and interval
    });
</script>

<svelte:window on:keydown={handleKeydown} />

<Loader />  

<main class="h-screen w-screen bg-[#0f0f16] text-white overflow-hidden relative font-sans flex flex-col">
  
    {#if view === 'landing' || showModal}
      <div class="absolute inset-0 z-50">
          <LandingModal on:select={view === 'landing' ? handleInitialSelection : handleModalSelection} />
      </div>
    {/if}

    {#if view === 'studio'}
        <Header bind:projectName={projectName}/>
      
        <TopToolbar
            bind:masterGain={masterGain} 
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
            <TrackList 
                bind:tracks={tracks} 
                on:requestAdd={handleAddRequest}
                on:select={handleTrackSelect}
                on:toggleMonitor={handleToggleMonitor}
                on:delete={handleDeleteTrack}
                on:openEq={handleOpenEq}
                on:openCompressor={handleOpenCompressor}
            />

            <Timeline 
                bind:tracks={tracks} 
                currentTime={currentTime} 
                bpm={bpm}
                timeSignatureNumerator={timeSignatureNumerator} 
                on:seek={(e) => seekTo(e.detail)}
                on:select={handleTrackSelect}
                on:refresh={refreshProjectState}
            /> 

        </div>

        <AIChatbot {tracks} />

    {/if}
    {#if showEqWindow}
          <EqWindow 
              trackId={eqTrackId} 
              onClose={() => showEqWindow = false} 
          />
    {/if}
    {#if showCompressorWindow}
        <CompressorWindow 
            trackId={compressorTrackId} 
            onClose={() => showCompressorWindow = false} 
        />
    {/if}

</main>