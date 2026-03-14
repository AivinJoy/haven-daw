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
    private polling = false;
    private intervalMs = 33; // ~30 FPS (stable for visual meters)

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

        // Prevent overlapping IPC calls
        if (!this.polling) {
            this.polling = true;

            try {
                const data: MeterSnapshot[] = await invoke('get_all_meters');

                /// Mutate existing state to prevent Svelte 5 reactive graph thrashing
                const activeIds = new Set<number>();

                for (const meter of data) {
                    this.levels[meter.track_id] = meter;
                    activeIds.add(meter.track_id);
                }

                // Clean up deleted tracks to prevent memory leaks
                for (const id of Object.keys(this.levels)) {
                    const numId = Number(id);
                    if (!activeIds.has(numId)) {
                        delete this.levels[numId];
                    }
                }

            } catch (e) {
                if (e !== "busy") {
                    console.error("Meter fetch failed:", e);
                }
            }

            this.polling = false;
        }

        // Stable throttled polling (not tied to display refresh rate)
        setTimeout(this.loop, this.intervalMs);
    };
}

export const meterStore = new MeterStore();