<script lang="ts">
    import { X, Mic, Speaker, Check } from 'lucide-svelte';
    import { ui } from '$lib/stores/ui.svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { onMount } from 'svelte';

    // TYPE DEFINITION: Matches the Rust struct
    type AudioDeviceInfo = {
        name: string;
        is_default: boolean;
    };

    let outputDevices = $state<AudioDeviceInfo[]>([]);
    let inputDevices = $state<AudioDeviceInfo[]>([]);
    
    let selectedOutput = $state("Default");
    let selectedInput = $state("Default");

    async function fetchDevices() {
        try {
            outputDevices = await invoke('get_output_devices');
            inputDevices = await invoke('get_input_devices');

            // --- AUTO-SELECT LOGIC ---
            // Find the device marked as 'is_default' by the backend
            const defOut = outputDevices.find(d => d.is_default);
            if (defOut) selectedOutput = defOut.name;

            const defIn = inputDevices.find(d => d.is_default);
            if (defIn) selectedInput = defIn.name;

        } catch (e) {
            console.error("Failed to load devices", e);
        }
    }

    onMount(() => {
        fetchDevices();
    });

    function close() {
        ui.isSettingsOpen = false;
    }
</script>

{#if ui.isSettingsOpen}
    <div 
        class="fixed inset-0 bg-black/50 z-99 backdrop-blur-sm transition-opacity"
        onclick={close}
        aria-hidden="true"
    ></div>

    <div class="fixed top-0 right-0 h-full w-80 bg-[#12121e] border-l border-white/10 z-100 shadow-2xl p-6 transform transition-transform duration-300 ease-out">
        
        <div class="flex items-center justify-between mb-8">
            <h2 class="text-xl font-bold text-white tracking-wide">Settings</h2>
            <button onclick={close} class="text-white/50 hover:text-white transition-colors">
                <X size={20} />
            </button>
        </div>

        <div class="space-y-8">
            
            <div class="space-y-3">
                <div class="flex items-center gap-2 text-brand-blue">
                    <Speaker size={18} />
                    <label for="output-device" class="text-sm font-semibold uppercase tracking-wider">Output Device</label>
                </div>
                
                <div class="relative">
                    <select 
                        id="output-device"
                        bind:value={selectedOutput}
                        class="w-full bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-white text-sm focus:outline-none focus:border-brand-blue appearance-none"
                    >
                        {#each outputDevices as device}
                            <option value={device.name} class="bg-black text-white">
                                {device.name} {device.is_default ? '(System Default)' : ''}
                            </option>
                        {/each}
                    </select>
                </div>
            </div>

            <div class="space-y-3">
                <div class="flex items-center gap-2 text-brand-blue">
                    <Mic size={18} />
                    <label for="input-device" class="text-sm font-semibold uppercase tracking-wider">Microphone Input</label>
                </div>
                
                <div class="relative">
                    <select 
                        id="input-device"
                        bind:value={selectedInput}
                        class="w-full bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-white text-sm focus:outline-none focus:border-brand-blue appearance-none"
                    >
                        {#each inputDevices as device}
                            <option value={device.name} class="bg-black text-white">
                                {device.name} {device.is_default ? '(System Default)' : ''}
                            </option>
                        {/each}
                    </select>
                </div>
            </div>

        </div>

        <div class="absolute bottom-6 left-6 right-6">
            <button 
                onclick={close}
                class="w-full bg-brand-blue hover:bg-blue-600 text-white font-semibold py-2 rounded-lg transition-colors flex items-center justify-center gap-2"
            >
                <Check size={16} />
                <span>Done</span>
            </button>
        </div>

    </div>
{/if}