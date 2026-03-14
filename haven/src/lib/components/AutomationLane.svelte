<script lang="ts">
    import { invoke } from '@tauri-apps/api/core'; 
    import { onMount } from 'svelte';

    type AutomationNode = { time: number; value: number };

    let { 
        trackId, 
        width, 
        height = 100, 
        pixelsPerSecond, 
    } = $props<{
        trackId: number; 
        width: number;
        height?: number;
        pixelsPerSecond: number;
    }>();

    let nodes = $state<AutomationNode[]>([]);

    let draggingIndex = $state<number | null>(null);
    let originalDragTime = $state<number | null>(null);

    const MAX_GAIN = 2.0;

    // --- Time Math is now incredibly simple (Seconds <-> Pixels) ---
    function timeToX(timeInSeconds: number): number {
        return timeInSeconds * pixelsPerSecond;
    }

    function xToTime(x: number): number {
        return x / pixelsPerSecond;
    }

    function gainToY(gain: number): number {
        const clampedGain = Math.max(0, Math.min(MAX_GAIN, gain));
        return height - ((clampedGain / MAX_GAIN) * height);
    }

    function yToGain(y: number): number {
        const clampedY = Math.max(0, Math.min(height, y));
        return MAX_GAIN - ((clampedY / height) * MAX_GAIN);
    }

    // --- Dynamic Polyline ---
    let polylinePoints = $derived.by(() => {
        if (nodes.length === 0) {
            const defaultY = gainToY(1.0);
            return `0,${defaultY} ${width},${defaultY}`;
        }

        const sorted = [...nodes].sort((a, b) => a.time - b.time);
        const first = sorted[0];
        const last = sorted[sorted.length - 1];

        let pts = `0,${gainToY(first.value)} `;
        pts += sorted.map(n => `${timeToX(n.time)},${gainToY(n.value)}`).join(' ');
        pts += ` ${width},${gainToY(last.value)}`;

        return pts;
    });

    async function loadNodes() {
        try {
            const fetchedNodes: AutomationNode[] = await invoke('get_volume_automation', { trackId });
            nodes = fetchedNodes.sort((a, b) => a.time - b.time);
        } catch (error) {
            console.error("Failed to load automation:", error);
        }
    }

    // --- Single Click to Add Node ---
    async function handleCanvasClick(e: PointerEvent) {
        // Guard: Don't add a node if they are clicking on an existing one
        if (e.target instanceof SVGCircleElement) return;

        const rect = (e.currentTarget as SVGElement).getBoundingClientRect();
        const x = e.clientX - rect.left;
        const y = e.clientY - rect.top;

        const time = xToTime(x);
        const value = yToGain(y);

        nodes = [...nodes, { time, value }].sort((a, b) => a.time - b.time);

        try {
            await invoke('add_volume_automation_node', { trackId, time, value });
        } catch (err) {
            console.error("Failed to add node:", err);
            loadNodes(); 
        }
    }

    function startDrag(index: number, e: PointerEvent) {
        e.stopPropagation(); // Prevents the canvas click from firing
        draggingIndex = index;
        originalDragTime = nodes[index].time;
        (e.target as Element).setPointerCapture(e.pointerId);
    }

    function onDrag(e: PointerEvent) {
        if (draggingIndex === null) return;
        const rect = (e.currentTarget as SVGElement).getBoundingClientRect();
        
        const x = Math.max(0, e.clientX - rect.left);
        const y = Math.max(0, Math.min(height, e.clientY - rect.top));

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

        nodes.sort((a, b) => a.time - b.time);
        nodes = [...nodes]; 

        try {
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
        // Always fetch from backend on mount to guarantee perfect precision
        loadNodes();
    });
</script>

<svg 
    class="automation-lane" 
    {width} 
    {height} 
    xmlns="http://www.w3.org/2000/svg"
    onpointerdown={handleCanvasClick}
    onpointermove={onDrag}
    role="application"
    aria-label="Automation Lane Canvas"
>
    <rect width="100%" height="100%" fill="transparent" />

    <polyline 
        points={polylinePoints} 
        fill="none" 
        stroke="#00FFCC" 
        stroke-width="5" 
        opacity="0.3" 
        style="filter: drop-shadow(0px 0px 4px #00FFCC);"
    />

    <polyline 
        points={polylinePoints} 
        fill="none" 
        stroke="#FFFFFF" 
        stroke-width="1.5" 
        opacity="0.9" 
    />

    {#each nodes as node, i}
        <circle 
            cx={timeToX(node.time)} 
            cy={gainToY(node.value)} 
            r={draggingIndex === i ? "6" : "4"} 
            fill="#FFFFFF" 
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
        pointer-events: auto;
        z-index: 25; 
    }

    .automation-node {
        cursor: grab;
        transition: r 0.1s ease;
    }

    .automation-node:active {
        cursor: grabbing;
    }

    .automation-node:hover {
        r: 6;
    }
</style>