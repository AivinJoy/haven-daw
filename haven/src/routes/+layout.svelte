<script lang="ts">
	import './layout.css';
	import favicon from '$lib/assets/favicon.svg';
	import Loader from '$lib/components/Loader.svelte';
	import SettingsSidebar from '$lib/components/SettingsSidebar.svelte';

	let { children } = $props();
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
</svelte:head>
<SettingsSidebar />

{@render children()}
