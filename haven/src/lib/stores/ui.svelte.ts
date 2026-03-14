// src/lib/stores/ui.svelte.ts
export class UIState {
    isSettingsOpen = $state(false);

    // --- NEW: Global Automation Toggle ---
    showAutomation = $state(false);

    toggleAutomation() {
        this.showAutomation = !this.showAutomation;
    }
}

export const ui = new UIState();