import { listen } from '@tauri-apps/api/event';

export class LoaderState {
    visible = $state(false);
    message = $state("Processing...");
    progress = $state(0);

    constructor() {
        // Automatically listen for Rust events when the app starts
        listen<any>('progress-update', (event) => {
            const { message, progress, visible } = event.payload;
            this.message = message;
            this.progress = progress;
            this.visible = visible;
        });
    }
}

// Export a single global instance
export const loader = new LoaderState();