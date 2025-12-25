// src/lib/managers/RecordingManager.ts
import { invoke } from '@tauri-apps/api/core';

export type RecordingCallback = (duration: number) => void;
const sleep = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));
export class RecordingManager {
    private isRecording = false;
    private pollInterval: number | null = null;
    private onUpdate: RecordingCallback | null = null;
    private currentFilePath = "";

    // NEW: Store context about where this recording belongs
    
    private trackId: number | null = null;
    private startTime: number = 0;

    constructor() {}

    /**
     * Starts recording to the specified file path.
     * @param filePath - The full path where the WAV file will be saved.
     * @param trackId - The ID of the track we are recording onto.
     * @param startTime - The timeline position (seconds) where recording started.
     * @param onUpdate - Callback to receive duration updates (e.g. for updating the UI).
     */
    async start(filePath: string, trackId: number, startTime: number, onUpdate: RecordingCallback) {
        if (this.isRecording) return;

        console.log(`🎙️ Starting Recording: ${filePath} on Track ${trackId}`);
        this.currentFilePath = filePath;
        this.trackId = trackId;
        this.startTime = startTime;
        this.onUpdate = onUpdate;
        
        try {
            await invoke('start_recording', { path: filePath });
            this.isRecording = true;
            this.startPolling();
        } catch (e) {
            console.error("Failed to start recording:", e);
            throw e;
        }
    }

    /**
     * Stops the recording and finalizes the file.
     * Returns the analysis data needed to create the final waveform clip.
     */
    /**
     * Stops the recording and finalizes the file.
     * Returns the analysis data needed to create the final waveform clip.
     */
    async stop() {
        if (!this.isRecording) return null;

        console.log("🛑 Stopping Recording...");
        this.isRecording = false;
        this.stopPolling();

        try {
            // 1. Stop the file writer (Flushes to disk)
            await invoke('stop_recording');
            
            // Wait for Windows to release the lock (Increased to 200ms for safety)
            await sleep(200);

            // --- STEP 2: ANALYZE FIRST (Swapped Order) ---
            // We run analysis first. If this works, we PROVE the file is valid and readable.
            console.log("Analyzing waveform...");
            const result = await invoke<{
                mins: number[], 
                maxs: number[], 
                duration: number 
            }>('analyze_file', { path: this.currentFilePath });
            // ----------------------------------------------

            // --- STEP 3: ADD TO ENGINE ---
            // Now that we know the file is good, we add it to the audio engine.
            if (this.trackId !== null) {
                console.log("Registering clip with engine...");
                try {
                    await invoke('add_clip', { 
                        trackId: this.trackId, 
                        path: this.currentFilePath, 
                        startTime: this.startTime 
                    });
                } catch (err) {
                    console.error("⚠️ Failed to add clip to engine:", err);
                }
            }

            console.log("✅ Recording Finalized:", result);
            return result;

        } catch (e) {
            console.error("❌ Critical: Failed to finalize recording:", e);
            throw e;
        }
    }

    private startPolling() {
        const loop = async () => {
            if (!this.isRecording) return;

            try {
                const status = await invoke<{ is_recording: boolean, duration: number }>('get_recording_status');
                
                if (!status.is_recording) {
                    this.isRecording = false;
                    return;
                }

                if (this.onUpdate) this.onUpdate(status.duration);
                this.pollInterval = requestAnimationFrame(loop);

            } catch (e) {
                console.error("Polling error:", e);
                this.isRecording = false;
            }
        };

        this.pollInterval = requestAnimationFrame(loop);
    }

    private stopPolling() {
        if (this.pollInterval) {
            cancelAnimationFrame(this.pollInterval);
            this.pollInterval = null;
        }
    }

    public get isActive() {
        return this.isRecording;
    }
}

export const recordingManager = new RecordingManager();