<script lang="ts">
    import { invoke } from '@tauri-apps/api/core'; 
    import { onMount } from 'svelte';

    let { 
        trackId, 
        width, 
        height = 100, 
        pixelsPerSecond, 
        sampleRate = 44100 
    } = $props<{
        trackId: string;
        width: number;
        height: number;
        pixelsPerSecond: number;
        sampleRate?: number;
    }>();

    type AutomationNode = { time: number; value: number };
    let nodes = $state<AutomationNode[]>([]);

    // --- Dragging State ---
    let draggingIndex = $state<number | null>(null);
    let originalDragTime = $state<number | null>(null);

    // --- Coordinate Mapping ---
    function timeToX(timeInSamples: number): number {
        return (timeInSamples / sampleRate) * pixelsPerSecond;
    }

    function xToTime(x: number): number {
        return Math.round((x / pixelsPerSecond) * sampleRate);
    }

    function gainToY(gain: number): number {
        const clampedGain = Math.max(0, Math.min(1, gain));
        return height - (clampedGain * height);
    }

    function yToGain(y: number): number {
        const clampedY = Math.max(0, Math.min(height, y));
        return 1.0 - (clampedY / height);
    }

    // Sort nodes dynamically so the polyline doesn't criss-cross if nodes overlap during drag
    let polylinePoints = $derived(
        [...nodes]
            .sort((a, b) => a.time - b.time)
            .map(n => `${timeToX(n.time)},${gainToY(n.value)}`)
            .join(' ')
    );

    // --- Backend Sync ---
    async function loadNodes() {
        try {
            nodes = await invoke('get_volume_automation', { trackId });
            nodes.sort((a, b) => a.time - b.time);
        } catch (error) {
            console.error("Failed to load automation:", error);
        }
    }

    // --- Mouse Interactions ---
    async function handleLaneDblClick(e: MouseEvent) {
        // Prevent adding a node if we double-clicked an existing circle
        if (e.target instanceof SVGCircleElement) return;

        const rect = (e.currentTarget as SVGElement).getBoundingClientRect();
        const x = e.clientX - rect.left;
        const y = e.clientY - rect.top;

        const time = xToTime(x);
        const value = yToGain(y);

        // Optimistic UI Update
        nodes = [...nodes, { time, value }].sort((a, b) => a.time - b.time);

        try {
            await invoke('add_volume_automation_node', { trackId, time, value });
        } catch (err) {
            console.error("Failed to add node:", err);
            loadNodes(); // Rollback on failure
        }
    }

    function startDrag(index: number, e: PointerEvent) {
        e.stopPropagation();
        draggingIndex = index;
        originalDragTime = nodes[index].time;
        // Capture the pointer so dragging works even if the mouse leaves the circle
        (e.target as Element).setPointerCapture(e.pointerId);
    }

    function onDrag(e: PointerEvent) {
        if (draggingIndex === null) return;
        
        const rect = (e.currentTarget as SVGElement).getBoundingClientRect();
        const x = Math.max(0, e.clientX - rect.left); // Prevent dragging off-screen left
        const y = Math.max(0, Math.min(height, e.clientY - rect.top));

        // Update local state smoothly for 60fps rendering
        nodes[draggingIndex] = {
            time: xToTime(x),
            value: yToGain(y)
        };
    }

    async function endDrag(e: PointerEvent) {
        if (draggingIndex === null || originalDragTime === null) return;

        const draggedNode = nodes[draggingIndex];
        const oldTime = originalDragTime;

        draggingIndex = null;
        originalDragTime = null;
        (e.target as Element).releasePointerCapture(e.pointerId);

        // Re-sort array based on new time
        nodes.sort((a, b) => a.time - b.time);
        nodes = [...nodes]; // Trigger reactivity

        try {
            // Because our Rust backend uses `time` as the sorted key, 
            // if the time changed, we must delete the old one and add the new one.
            if (oldTime !== draggedNode.time) {
                await invoke('remove_volume_automation_node', { trackId, time: oldTime });
            }
            await invoke('add_volume_automation_node', { trackId, time: draggedNode.time, value: draggedNode.value });
        } catch (err) {
            console.error("Failed to update node:", err);
            loadNodes();
        }
    }

    async function handleRightClick(index: number, e: MouseEvent) {
        e.preventDefault();
        e.stopPropagation();

        const nodeTime = nodes[index].time;
        
        // Optimistic remove
        nodes.splice(index, 1);
        nodes = [...nodes];

        try {
            await invoke('remove_volume_automation_node', { trackId, time: nodeTime });
        } catch (err) {
            console.error("Failed to delete node:", err);
            loadNodes();
        }
    }

    onMount(() => {
        loadNodes();
    });
</script>

<svg 
    class="automation-lane" 
    {width} 
    {height} 
    xmlns="http://www.w3.org/2000/svg"
    ondblclick={handleLaneDblClick}
    onpointermove={onDrag}
    role="application"
    aria-label="Automation Lane Canvas"
>
    <rect width="100%" height="100%" fill="transparent" />

    {#if nodes.length > 1}
        <polyline 
            points={polylinePoints} 
            fill="none" 
            stroke="#00FFCC" 
            stroke-width="2" 
            opacity="0.8" 
        />
    {/if}

    {#each nodes as node, i}
        <circle 
            cx={timeToX(node.time)} 
            cy={gainToY(node.value)} 
            r={draggingIndex === i ? "6" : "4"} 
            fill={draggingIndex === i ? "#00FFCC" : "#FFFFFF"} 
            stroke="#00FFCC" 
            stroke-width="2" 
            class="automation-node"
            onpointerdown={(e) => startDrag(i, e)}
            onpointerup={endDrag}
            oncontextmenu={(e) => handleRightClick(i, e)}
            role="button"
            tabindex="0"
            aria-label="Automation Node"
        />
    {/each}
</svg>

<style>
    .automation-lane {
        position: absolute;
        top: 0;
        left: 0;
        /* Must be auto to receive clicks/drags */
        pointer-events: auto; 
        z-index: 25; /* Above clips, below menus */
    }

    .automation-node {
        cursor: grab;
        transition: r 0.1s ease, fill 0.1s ease;
    }

    .automation-node:active {
        cursor: grabbing;
    }

    .automation-node:hover {
        r: 6;
    }
</style>