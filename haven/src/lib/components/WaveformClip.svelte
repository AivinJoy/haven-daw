<!-- haven\src\lib\components\WaveformClip.svelte -->
<script lang="ts">
  import { onMount } from 'svelte';

  interface WaveformData {
    mins: number[];
    maxs: number[];
    duration: number;
    binsPerSecond?: number;
    bpm?: number;
  }

  let { 
    color = 'bg-brand-blue', 
    waveform = null, 
    currentTime = 0,    
    startTime = 0,      
    duration = 0,
    offset = 0,
    zoom = 1,
    name = "Audio Clip"
  } = $props();

  let canvas: HTMLCanvasElement;
  let container: HTMLDivElement;
  let resizeObserver: ResizeObserver;

  // --- CACHING ---
  let offscreenCanvas: HTMLCanvasElement | null = null;
  let cachedZoom = -1;
  let cachedWidth = -1;
  let cachedHeight = -1;
  let cachedColor = '';
  let cachedOffset = -1;   // <--- Cache Check
  let cachedDuration = -1; // <--- Cache Check
  
  let cachedWaveformRef: WaveformData | null | any = null; // Track actual object reference

  const PIXELS_PER_SECOND = 50;

  // Reverted to Solid Colors (No Dim/Bright split)
  const colorPalette: Record<string, string> = {
    'bg-brand-blue':  '#3b82f6', 
    'bg-brand-red':   '#ef4444', 
    'bg-purple-500':  '#a855f7',
    'bg-emerald-500': '#10b981',
    'bg-orange-500':  '#f97316',
    'bg-pink-500':    '#ec4899',
    'bg-cyan-500':    '#06b6d4', // Added
    'bg-indigo-500':  '#6366f1', // Added
    'bg-rose-500':    '#f43f5e'  // Added
  };

  const waveColor = $derived(colorPalette[color] || '#3b82f6');

  const renderStaticWaveform = (width: number, height: number, waveColor: string) => {
      // Basic validation
      if (!waveform || !waveform.mins || waveform.mins.length === 0) return null;

      const c = document.createElement('canvas');
      c.width = width;
      c.height = height;
      const ctx = c.getContext('2d');
      if (!ctx) return null;

      // --- ROBUST SLICING LOGIC ---
      const totalBins = waveform.maxs.length;

      // Prefer backend-provided bins/sec. Fallbacks are still kept.
      let binsPerSecond = waveform.binsPerSecond ?? 100;
      if ((!waveform.binsPerSecond || waveform.binsPerSecond <= 0) && waveform.duration > 0) {
        // binsPerSecond = totalBins / waveform.duration;

        const rawBps = totalBins / waveform.duration;

        if (Math.abs(rawBps - Math.round(rawBps)) < 0.1) {
          binsPerSecond = Math.round(rawBps);
        }else {
          binsPerSecond = rawBps;
        }

      }

      const startBinIndex = Math.floor(offset * binsPerSecond);
      const endBinIndex = Math.floor((offset + duration) * binsPerSecond);


      // 3. Slice the data (Clamp to bounds)
      const safeStart = Math.max(0, Math.min(startBinIndex, totalBins));
      const safeEnd = Math.max(safeStart, Math.min(endBinIndex, totalBins));

      // 4. Create Slices
      const sliceMins = waveform.mins.slice(safeStart, safeEnd);
      const sliceMaxs = waveform.maxs.slice(safeStart, safeEnd);
      
      const len = sliceMaxs.length;
      if (len < 2) return c; // Nothing to draw

      const centerY = height / 2;
      const step = width / (len - 1); 

      ctx.resetTransform();
      // ctx.scale(window.devicePixelRatio || 1, window.devicePixelRatio || 1);

      ctx.beginPath();
      ctx.moveTo(0, centerY);

      // Top (Maxs)
      for (let i = 0; i < len - 1; i++) {
          const x = i * step;
          const y = centerY - (sliceMaxs[i] * centerY);
          const nextX = (i + 1) * step;
          const nextY = centerY - (sliceMaxs[i+1] * centerY);
          const midX = (x + nextX) / 2;
          const midY = (y + nextY) / 2;
          if (i === 0) ctx.lineTo(x, y);
          ctx.quadraticCurveTo(x, y, midX, midY);
      }
      
      // Bottom (Mins)
      for (let i = len - 1; i > 0; i--) {
          const x = i * step;
          const y = centerY - (sliceMins[i] * centerY); 
          const prevX = (i - 1) * step;
          const prevY = centerY - (sliceMins[i-1] * centerY);
          const midX = (x + prevX) / 2;
          const midY = (y + prevY) / 2;
          ctx.quadraticCurveTo(x, y, midX, midY);
      }
      ctx.lineTo(0, centerY - (sliceMins[0] * centerY));
      ctx.closePath();

      ctx.fillStyle = waveColor;
      ctx.fill();
      console.log("WF", { offset, duration, wfDur: waveform.duration, binsPerSecond: waveform.binsPerSecond, bins: waveform.maxs.length });

      return c;
      
  };
  

  const draw = () => {
    if (!canvas || !container || !waveform) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    // Calculate Render Size
    const exactPixelWidth = Math.max(1, duration * PIXELS_PER_SECOND * zoom);
    const rect = container.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    
    // Safety check for container size
    if (rect.width === 0 || rect.height === 0) return;

    const renderWidth = Math.ceil(exactPixelWidth * dpr);
    const renderHeight = Math.ceil(rect.height * dpr);

    if (canvas.width !== renderWidth || canvas.height !== renderHeight) {
        canvas.width = renderWidth;
        canvas.height = renderHeight;
    }

    const waveColor = colorPalette[color] || '#3b82f6';

    // Re-render if ANY property changed
    if (!offscreenCanvas || 
        Math.abs(cachedWidth - renderWidth) > 1 || 
        Math.abs(cachedHeight - renderHeight) > 1 ||
        Math.abs(cachedZoom - zoom) > 0.001 ||
        cachedColor !== color ||
        Math.abs(cachedOffset - offset) > 0.01 ||
        Math.abs(cachedDuration - duration) > 0.01 ||
        cachedWaveformRef !== waveform // Check if the waveform object itself was swapped
    ) {
        offscreenCanvas = renderStaticWaveform(renderWidth, renderHeight, waveColor);
        cachedWidth = renderWidth;
        cachedHeight = renderHeight;
        cachedZoom = zoom;
        cachedColor = color;
        cachedOffset = offset;
        cachedDuration = duration;
        cachedWaveformRef = waveform;
    }

    ctx.resetTransform();
    ctx.clearRect(0, 0, canvas.width, canvas.height);

    if (offscreenCanvas) {
        ctx.drawImage(offscreenCanvas, 0, 0);
    }
  };

  $effect(() => {
     // Re-run draw when any dependency changes
     if (waveform && container && (zoom || duration || offset || color)) {
         requestAnimationFrame(draw);
     }
  });

  onMount(() => {
    resizeObserver = new ResizeObserver(() => window.requestAnimationFrame(draw));
    if (container) resizeObserver.observe(container);
    return () => resizeObserver?.disconnect();
  });
</script>

<div 
  bind:this={container}
  class="relative h-[85%] rounded-md overflow-hidden select-none ring-1 ring-white/10"
  style="background-color: rgba(255,255,255,0.02); width: 100%;"
>
  <canvas bind:this={canvas} class="absolute top-0 left-0 w-full h-full z-10 opacity-90"></canvas>
  
  <!-- <span class="absolute top-1 left-2 text-[10px] ... text-black/60 ...">
   {name}
</span> -->

<span 
    class="absolute top-1 left-2 text-[10px] font-bold font-sans z-20 truncate max-w-[95%] pointer-events-none"
    style="color: {waveColor}; filter: brightness(0.6) saturate(1.5);"
>
   {name}
</span>
</div>