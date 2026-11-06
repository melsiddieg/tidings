<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import { store, type ContextMenu } from '../lib/store.svelte';

  let { menu }: { menu: ContextMenu } = $props();

  let el: HTMLDivElement | undefined = $state();
  let x = $state(untrack(() => menu.x));
  let y = $state(untrack(() => menu.y));

  // Keep the menu inside the window.
  $effect(() => {
    if (!el) return;
    const r = el.getBoundingClientRect();
    x = Math.min(menu.x, window.innerWidth - r.width - 8);
    y = menu.y + r.height > window.innerHeight - 8 ? Math.max(8, menu.y - r.height) : menu.y;
  });

  onMount(() => {
    const close = (e: Event) => {
      if (e.type === 'mousedown' && el?.contains(e.target as Node)) return;
      store.menu = null;
    };
    window.addEventListener('mousedown', close, true);
    window.addEventListener('blur', close);
    window.addEventListener('resize', close);
    window.addEventListener('wheel', close, { passive: true });
    return () => {
      window.removeEventListener('mousedown', close, true);
      window.removeEventListener('blur', close);
      window.removeEventListener('resize', close);
      window.removeEventListener('wheel', close);
    };
  });
</script>

<div class="menu" bind:this={el} style:left="{x}px" style:top="{y}px" role="menu">
  {#each menu.items as item, i (i)}
    {#if item.separator}
      <div class="separator"></div>
    {:else}
      <button
        class="item"
        class:danger={item.danger}
        role="menuitem"
        disabled={item.disabled}
        onclick={() => {
          store.menu = null;
          item.action?.();
        }}
      >
        <span class="icon-slot">
          {#if item.checked}
            <Icon name="check" size={14} stroke={2.4} />
          {:else if item.icon && item.checked === undefined}
            <Icon name={item.icon} size={14} />
          {/if}
        </span>
        <span class="label">{item.label}</span>
        {#if item.shortcut}<span class="shortcut">{item.shortcut}</span>{/if}
      </button>
    {/if}
  {/each}
</div>

<style>
  .menu {
    position: fixed;
    z-index: 1000;
    min-width: 210px;
    max-width: 320px;
    padding: 5px;
    border-radius: 11px;
    background: var(--bg-elevated);
    box-shadow: var(--shadow-lg);
    animation: pop 0.12s var(--ease-out);
    transform-origin: top left;
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: scale(0.97);
    }
  }
  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 28px;
    padding: 0 10px 0 8px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--text);
    font-size: 13px;
    text-align: left;
  }
  .item:hover:not(:disabled) {
    background: var(--accent);
    color: #fff;
  }
  .item:hover:not(:disabled) .icon-slot,
  .item:hover:not(:disabled) .shortcut {
    color: rgba(255, 255, 255, 0.85);
  }
  :global([data-theme='dark']) .item:hover:not(:disabled),
  :global([data-theme='dark']) .item:hover:not(:disabled) .icon-slot,
  :global([data-theme='dark']) .item:hover:not(:disabled) .shortcut {
    color: #141416;
  }
  .item:disabled {
    opacity: 0.4;
  }
  .item.danger {
    color: var(--danger);
  }
  .item.danger:hover:not(:disabled) {
    background: var(--danger);
    color: #fff;
  }
  .icon-slot {
    display: grid;
    place-items: center;
    width: 16px;
    color: var(--text-2);
    flex: none;
  }
  .item.danger .icon-slot {
    color: inherit;
  }
  .label {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .shortcut {
    font-size: 11.5px;
    color: var(--text-3);
  }
  .separator {
    height: 1px;
    margin: 4px 8px;
    background: var(--border);
  }
</style>
