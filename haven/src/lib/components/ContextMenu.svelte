<script lang="ts">
  import { onMount, onDestroy } from "svelte";

  export let x = 0;
  export let y = 0;

  type ContextOption = {
    label: string;
    action: () => void;
    danger?: boolean;
    disabled?: boolean;
  };

  export let options: ContextOption[] = [];
  export let onClose: () => void;

  let menuElement: HTMLDivElement;

  function handleClickOutside(event: MouseEvent) {
    if (menuElement && !menuElement.contains(event.target as Node)) onClose();
  }

  onMount(() => {
    document.addEventListener("mousedown", handleClickOutside);
    const rect = menuElement.getBoundingClientRect();
    if (x + rect.width > window.innerWidth) x -= rect.width;
    if (y + rect.height > window.innerHeight) y -= rect.height;
  });

  onDestroy(() => {
    document.removeEventListener("mousedown", handleClickOutside);
  });
</script>

<div
  bind:this={menuElement}
  class="fixed z-50 bg-[#1e1e2e] border border-white/10 rounded shadow-xl py-1 w-48 font-sans text-sm"
  style="top: {y}px; left: {x}px;"
>
  {#each options as option}
    <button
      disabled={option.disabled}
      class="w-full text-left px-4 py-2 transition-colors flex items-center gap-2
        {option.danger ? 'text-red-400' : 'text-gray-200'}
        {option.disabled ? 'opacity-40 cursor-not-allowed' : 'hover:bg-white/5'}"
      on:click={() => {
        if (option.disabled) return; // IMPORTANT: do nothing and don't close
        option.action();
        onClose();
      }}
    >
      {option.label}
    </button>
  {/each}
</div>
