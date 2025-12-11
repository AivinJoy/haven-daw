<script lang="ts">
  import { onMount } from 'svelte';

  let { 
    color = 'bg-brand-blue', 
    waveform = null, 
    currentTime = 0,    
    startTime = 0,      
    duration = 0,
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

  const PIXELS_PER_SECOND = 50;

  // Reverted to Solid Colors (No Dim/Bright split)
  const colorPalette: Record<string, string> = {
    'bg-brand-blue':  '#3b82f6', 
    'bg-brand-red':   '#ef4444', 
    'bg-purple-500':  '#a855f7',
    'bg-emerald-500': '#10b981',
    'bg-orange-500':  '#f97316',
    'bg-pink-500':    '#ec4899'
  };

  const renderStaticWaveform = (width: number, height: number, waveColor: string) => {
      if (!waveform || !waveform.mins) return null;
      
      const c = document.createElement('canvas');
      c.width = width;
      c.height = height;
      const ctx = c.getContext('2d');
      if (!ctx) return null;

      const { mins, maxs } = waveform;
      const len = maxs.length;
      const centerY = height / 2;
      
      const logicalWidth = width / (window.devicePixelRatio || 1);
      const step = logicalWidth / Math.max(1, len - 1); 

      ctx.resetTransform();
      ctx.scale(window.devicePixelRatio || 1, window.devicePixelRatio || 1);

      ctx.beginPath();
      ctx.moveTo(0, centerY);

      // Top
      for (let i = 0; i < len - 1; i++) {
          const x = i * step;
          const y = centerY - (maxs[i] * centerY);
          const nextX = (i + 1) * step;
          const nextY = centerY - (maxs[i+1] * centerY);
          const midX = (x + nextX) / 2;
          const midY = (y + nextY) / 2;
          if (i === 0) ctx.lineTo(x, y);
          ctx.quadraticCurveTo(x, y, midX, midY);
      }
      
      // Bottom
      for (let i = len - 1; i > 0; i--) {
          const x = i * step;
          const y = centerY - (mins[i] * centerY); 
          const prevX = (i - 1) * step;
          const prevY = centerY - (mins[i-1] * centerY);
          const midX = (x + prevX) / 2;
          const midY = (y + prevY) / 2;
          ctx.quadraticCurveTo(x, y, midX, midY);
      }
      ctx.lineTo(0, centerY - (mins[0] * centerY));
      ctx.closePath();

      ctx.fillStyle = waveColor;
      ctx.fill();

      return c;
  };

  const draw = () => {
    if (!canvas || !container || !waveform) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const exactPixelWidth = duration * PIXELS_PER_SECOND * zoom;
    const rect = container.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    
    const renderWidth = Math.ceil(exactPixelWidth * dpr);
    const renderHeight = Math.ceil(rect.height * dpr);

    if (canvas.width !== renderWidth || canvas.height !== renderHeight) {
        canvas.width = renderWidth;
        canvas.height = renderHeight;
    }

    const waveColor = colorPalette[color] || '#3b82f6';

    if (!offscreenCanvas || 
        Math.abs(cachedWidth - renderWidth) > 1 || 
        Math.abs(cachedHeight - renderHeight) > 1 ||
        Math.abs(cachedZoom - zoom) > 0.001 ||
        cachedColor !== color
    ) {
        offscreenCanvas = renderStaticWaveform(renderWidth, renderHeight, waveColor);
        cachedWidth = renderWidth;
        cachedHeight = renderHeight;
        cachedZoom = zoom;
        cachedColor = color;
    }

    ctx.resetTransform();
    ctx.clearRect(0, 0, canvas.width, canvas.height);

    if (offscreenCanvas) {
        ctx.drawImage(offscreenCanvas, 0, 0);
    }

    // --- REMOVED THE OVERLAY BLOCK HERE ---
    // The waveform is now static single-color.
  };

  $effect(() => {
     if (waveform && container) {
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
  <canvas bind:this={canvas} class="absolute top-0 left-0 h-full z-10 opacity-90"></canvas>
  
  <span class="absolute top-1 left-2 text-[10px] font-bold font-sans text-black/60 z-20 truncate max-w-[95%] mix-blend-hard-light drop-shadow-sm pointer-events-none">
    {name}
  </span>
</div>