// src/lib/stores/ui.svelte.ts
export class UIState {
    isSettingsOpen = $state(false);
}

export const ui = new UIState();