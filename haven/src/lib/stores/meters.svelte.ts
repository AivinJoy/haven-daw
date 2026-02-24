// src/lib/stores/meters.svelte.ts
import { invoke } from '@tauri-apps/api/core';

export type MeterSnapshot = {
    track_id: number;
    peak_l: number;
    peak_r: number;
    hold_l: number;
    hold_r: number;
    rms_l: number;
    rms_r: number;
};

class MeterStore {
    // Svelte 5 reactive state holding the latest meters by Track ID
    levels = $state<Record<number, MeterSnapshot>>({});
    private running = false;

    start() {
        if (this.running) return;
        this.running = true;
        this.loop();
    }

    stop() {
        this.running = false;
    }

    private loop = async () => {
        if (!this.running) return;

        try {
            // 1 Call per frame, fetches ALL tracks instantly lock-free
            const data: MeterSnapshot[] = await invoke('get_all_meters');
            
            for (const meter of data) {
                this.levels[meter.track_id] = meter;
            }
        } catch (e) {
            console.error("Meter fetch failed:", e);
        }

        // Recursively call the next frame (locks to monitor refresh rate, e.g., 60fps)
        requestAnimationFrame(this.loop);
    }
}

export const meterStore = new MeterStore();