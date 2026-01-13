<script lang="ts">
  interface Props {
    value: number;
    min: number;
    max: number;
    step?: number;
    size?: 'sm' | 'lg';
    color: string; // HEX CODE (e.g., "#fbbf24")
    mapMode?: 'linear' | 'log';
    onChange: (val: number) => void;
  }

  let { 
    value = $bindable(), 
    min, 
    max, 
    step = 0.1, 
    size = 'lg', 
    color, 
    mapMode = 'linear', 
    onChange 
  }: Props = $props();

  // --- CONFIG ---
  let config = $derived(size === 'lg' 
    ? { px: 80, stroke: 4, r: 34, indicatorH: 25, tickH: 6, tickW: 2, tickOffset: 8 } 
    : { px: 40, stroke: 3, r: 16, indicatorH: 30, tickH: 3, tickW: 1.5, tickOffset: 5 } 
  );

  // --- INTERACTION STATE ---
  let isDragging = $state(false);
  let startY = 0;
  let startValue = 0;

  // --- LOGARITHMIC HELPERS ---
  function toProgress(val: number) {
    if (mapMode === 'linear') return (val - min) / (max - min);
    return (Math.log(val) - Math.log(min)) / (Math.log(max) - Math.log(min));
  }

  function fromProgress(p: number) {
    if (mapMode === 'linear') return min + p * (max - min);
    return min * Math.pow(max / min, p);
  }

  // --- DRAG LOGIC ---
  function onMouseDown(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    isDragging = true;
    startY = e.clientY;
    startValue = value;
    window.addEventListener('mousemove', onMouseMove);
    window.addEventListener('mouseup', onMouseUp);
  }

  function onMouseMove(e: MouseEvent) {
    if (!isDragging) return;
    e.preventDefault();

    const sensitivity = 200; 
    const deltaY = startY - e.clientY;
    const speed = e.shiftKey ? 0.2 : 1.0;

    let currentProgress = toProgress(startValue);
    let newProgress = currentProgress + (deltaY / sensitivity) * speed;
    newProgress = Math.max(0, Math.min(1, newProgress));
    let nextValue = fromProgress(newProgress);

    if (step > 0 && mapMode === 'linear') {
      nextValue = Math.round(nextValue / step) * step;
    } else if (mapMode === 'log') {
        nextValue = Math.round(nextValue);
    }

    value = nextValue;
    onChange(value);
  }

  function onMouseUp() {
    isDragging = false;
    window.removeEventListener('mousemove', onMouseMove);
    window.removeEventListener('mouseup', onMouseUp);
  }

  // --- VISUAL MATH ---
  let fraction = $derived(Math.max(0, Math.min(1, toProgress(value))));
  let angle = $derived(fraction * 270 - 135);

  let circumference = $derived(2 * Math.PI * config.r);
  let arcLength = $derived(circumference * 0.75);
  let valueDash = $derived(arcLength * fraction);
  let gapDash = $derived(circumference - valueDash);

  const ticks = Array.from({ length: 21 }, (_, i) => {
    const p = i / 20; 
    const deg = p * 270 - 135; 
    return deg;
  });

</script>

<div 
  class="relative flex items-center justify-center select-none cursor-ns-resize group touch-none"
  style={`width: ${config.px}px; height: ${config.px}px; color: ${color};`} 
  onmousedown={onMouseDown}
  role="slider"
  aria-valuenow={value}
  tabindex="0"
>
  
  <div class="absolute inset-0 pointer-events-none opacity-60">
    {#each ticks as deg}
      <div 
        class="absolute bg-[#3f3f46] rounded-full left-1/2 top-1/2 origin-top"
        style={`
            width: ${config.tickW}px; 
            height: ${config.tickH}px; 
            transform: translate(-50%, -50%) rotate(${deg}deg) translateY(-${config.px/2 + config.tickOffset}px);
        `}
      ></div>
    {/each}
  </div>

  {#if size === 'lg'}
  <svg width={config.px} height={config.px} class="absolute inset-0 rotate-135 pointer-events-none">
    <circle
      cx={config.px / 2} cy={config.px / 2} r={config.r}
      fill="none" 
      stroke="#27272a" 
      stroke-width={config.stroke}
      stroke-linecap="round"
      stroke-dasharray={`${arcLength} ${circumference}`}
    />
    <circle
      cx={config.px / 2} cy={config.px / 2} r={config.r}
      fill="none" 
      stroke={color} 
      stroke-width={config.stroke}
      stroke-linecap="round"
      stroke-dasharray={`${valueDash} ${gapDash}`}
      class="drop-shadow-[0_0_3px_currentColor] transition-none" 
    />
  </svg>
  {/if}

  <div 
    class="absolute rounded-full bg-eq-panel shadow-[0_5px_15px_rgba(0,0,0,0.5),inset_0_1px_1px_rgba(255,255,255,0.1)] flex items-start justify-center pointer-events-none border border-black/50"
    style={`
        width: ${config.px * 0.7}px; 
        height: ${config.px * 0.7}px; 
        transform: rotate(${angle}deg);
    `}
  >
    <div 
      class="w-0.5 mt-1 rounded-full shadow-[0_0_2px_currentColor]"
      style={`height: ${config.indicatorH}%; background-color: ${color};`}
    ></div>
  </div>

</div>