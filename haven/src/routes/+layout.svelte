<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
    import { meterStore } from '$lib/stores/meters.svelte';
	import './layout.css';
	import favicon from '$lib/assets/favicon.svg';
	import Loader from '$lib/components/Loader.svelte';
	import SettingsSidebar from '$lib/components/SettingsSidebar.svelte';

	let { children } = $props();
	// --- ADDED: Manage the lock-free polling loop ---
    onMount(() => {
        meterStore.start();
    });

    onDestroy(() => {
        meterStore.stop();
    });
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
</svelte:head>
<SettingsSidebar />

{@render children()}
