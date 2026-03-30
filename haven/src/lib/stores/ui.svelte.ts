// src/lib/stores/ui.svelte.ts
export class UIState {
    isSettingsOpen = $state(false);

    // --- Global Automation Toggle ---
    showAutomation = $state(false);

    // --- NEW: Effect Window Tracking ---
    openEqTrackId = $state<number | null>(null);
    openCompressorTrackId = $state<number | null>(null);
    openReverbTrackId = $state<number | null>(null);

    toggleAutomation() {
        this.showAutomation = !this.showAutomation;
    }

    // Helper to easily close all effect windows at once
    closeEffectWindows() {
        this.openEqTrackId = null;
        this.openCompressorTrackId = null;
        this.openReverbTrackId = null;
    }
}

export const ui = new UIState();