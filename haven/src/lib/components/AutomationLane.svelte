<script module>
    let activeLaneId: number | null = null;
</script>

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
    let hoveredIndex = $state<number | null>(null);
    let activeIndex = $derived(draggingIndex !== null ? draggingIndex : hoveredIndex);

    let historyPast = $state<AutomationNode[][]>([]);
    let historyFuture = $state<AutomationNode[][]>([]);

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

    function gainToDbString(gain: number): string {
        if (gain <= 0.0001) return "-∞ dB"; // Handle complete silence
        const db = 20 * Math.log10(gain);
        const sign = db > 0 ? "+" : "";
        return `${sign}${db.toFixed(1)} dB`;
    }

    // --- UNDO/REDO LOGIC ---
    function saveState() {
        historyPast = [...historyPast, JSON.parse(JSON.stringify(nodes))];
        if (historyPast.length > 50) historyPast.shift(); // Keep last 50 actions
        historyFuture = [];
    }

    async function syncNodesToBackend(targetNodes: AutomationNode[]) {
        try {
            const currentBackendNodes: AutomationNode[] = await invoke('get_volume_automation', { trackId });
            
            // Allow a tiny margin of error for f32 (Rust) vs f64 (JS) math
            const EPSILON = 0.001; 
            
            const findMatch = (nodesList: AutomationNode[], time: number) => 
                nodesList.find(n => Math.abs(n.time - time) < EPSILON);

            // 1. Remove nodes from backend that are NOT in target state
            for (const backendNode of currentBackendNodes) {
                const targetMatch = findMatch(targetNodes, backendNode.time);
                if (!targetMatch || Math.abs(targetMatch.value - backendNode.value) > EPSILON) {
                    await invoke('remove_volume_automation_node', { trackId, time: backendNode.time });
                }
            }

            // 2. Add nodes to backend that are NEW or CHANGED
            for (const targetNode of targetNodes) {
                const backendMatch = findMatch(currentBackendNodes, targetNode.time);
                if (!backendMatch || Math.abs(backendMatch.value - targetNode.value) > EPSILON) {
                    await invoke('add_volume_automation_node', { trackId, time: targetNode.time, value: targetNode.value });
                }
            }
        } catch (err) {
            console.error("Failed to sync undo/redo state:", err);
        } finally {
            loadNodes(); 
        }
    }

    async function undo() {
        if (historyPast.length === 0) return;
        const previous = historyPast.pop()!;
        historyFuture = [...historyFuture, JSON.parse(JSON.stringify(nodes))];
        nodes = previous;
        await syncNodesToBackend(nodes);
    }

    async function redo() {
        if (historyFuture.length === 0) return;
        const next = historyFuture.pop()!;
        historyPast = [...historyPast, JSON.parse(JSON.stringify(nodes))];
        nodes = next;
        await syncNodesToBackend(nodes);
    }

    function handleKeyDown(e: KeyboardEvent) {
        // Now it checks if THIS lane is the currently active one for the DAW
        if (activeLaneId !== trackId) return;
        
        const isMac = navigator.userAgent.includes('Mac');
        const cmdOrCtrl = isMac ? e.metaKey : e.ctrlKey;
        
        if (cmdOrCtrl && e.key.toLowerCase() === 'z') {
            e.preventDefault();
            if (e.shiftKey) redo();
            else undo();
        } else if (cmdOrCtrl && e.key.toLowerCase() === 'y') {
            e.preventDefault();
            redo();
        }
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
        activeLaneId = trackId;
        saveState();

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
        activeLaneId = trackId;
        saveState();
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
        activeLaneId = trackId;
        saveState();

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

    async function handleDoubleClick(index: number, e: MouseEvent) {
        e.preventDefault();
        e.stopPropagation();
        activeLaneId = trackId;
        saveState();

        const node = nodes[index];
        const resetValue = 1.0; // 1.0 is 0 dB (Unity Gain)

        // Optimistically update UI
        nodes[index] = { ...node, value: resetValue };
        nodes = [...nodes];

        try {
            await invoke('add_volume_automation_node', { 
                trackId, 
                time: node.time, 
                value: resetValue 
            });
        } catch (err) {
            console.error("Failed to reset node:", err);
            loadNodes();
        }
    }

    onMount(() => {
        // Always fetch from backend on mount to guarantee perfect precision
        loadNodes();
    });
</script>

<svelte:window onkeydown={handleKeyDown} />

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
            stroke="#FFFFFF" 
            stroke-width="2" 
            class="automation-node"
            onpointerdown={(e) => startDrag(i, e)}
            onpointerup={endDrag}
            oncontextmenu={(e) => handleRightClick(i, e)}
            ondblclick={(e) => handleDoubleClick(i, e)}
            onpointerenter={() => hoveredIndex = i}
            onpointerleave={() => hoveredIndex = null}
            role="button"
            tabindex="0"
            aria-label="Automation Node"
        />
    {/each}

    {#if activeIndex !== null && nodes[activeIndex]}
        {@const node = nodes[activeIndex]}
        {@const x = timeToX(node.time)}
        {@const y = gainToY(node.value)}
        {@const tooltipY = y < 30 ? y + 25 : y - 25}
        
        <g transform="translate({x}, {tooltipY})" style="pointer-events: none;">
            <rect 
                x="-32" y="-12" 
                width="64" height="24" rx="4" 
                fill="#0a0a0f" 
                stroke="rgba(255,255,255,0.15)" 
                stroke-width="1" 
                style="filter: drop-shadow(0px 4px 10px rgba(0,0,0,0.5));" 
            />
            <text 
                x="0" y="3" 
                font-family="monospace" 
                font-size="10" 
                fill="#00FFCC" 
                text-anchor="middle" 
                dominant-baseline="middle" 
                font-weight="bold"
            >
                {gainToDbString(node.value)}
            </text>
        </g>
    {/if}
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