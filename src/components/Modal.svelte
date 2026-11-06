<script lang="ts">
  import type { Snippet } from 'svelte';
  import { fade, scale } from 'svelte/transition';
  import Icon from '../lib/Icon.svelte';
  import { store } from '../lib/store.svelte';

  let {
    title,
    width = 460,
    children,
    footer,
    padded = true,
  }: { title?: string; width?: number; children: Snippet; footer?: Snippet; padded?: boolean } = $props();

  let box: HTMLDivElement | undefined = $state();

  // Focus the first field when the dialog opens.
  $effect(() => {
    const first = box?.querySelector<HTMLElement>('input:not([type=hidden]), select, textarea, [data-autofocus]');
    first?.focus();
  });

  function close() {
    store.dialog = null;
  }
</script>

<div class="backdrop" transition:fade={{ duration: 140 }} role="presentation" onmousedown={(e) => e.target === e.currentTarget && close()}>
  <div class="modal" bind:this={box} style:width="{width}px" role="dialog" aria-modal="true" aria-label={title} transition:scale={{ start: 0.97, duration: 160 }}>
    {#if title}
      <header>
        <h2>{title}</h2>
        <button class="icon-btn" aria-label="Close" onclick={close}><Icon name="x" size={16} /></button>
      </header>
    {/if}
    <div class="body" class:padded>
      {@render children()}
    </div>
    {#if footer}
      <footer>{@render footer()}</footer>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 900;
    display: grid;
    place-items: center;
    padding: 24px;
    background: var(--scrim);
    -webkit-backdrop-filter: blur(3px);
    backdrop-filter: blur(3px);
  }
  .modal {
    max-width: 100%;
    max-height: calc(100vh - 48px);
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    border-radius: var(--radius-lg);
    background: var(--bg-elevated);
    box-shadow: var(--shadow-lg);
    overflow: hidden;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 14px 4px 22px;
  }
  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 700;
    letter-spacing: -0.01em;
  }
  .body {
    overflow-y: auto;
    min-height: 0;
  }
  .body.padded {
    padding: 14px 22px 20px;
  }
  footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    padding: 14px 18px;
    border-top: 1px solid var(--border);
    background: var(--bg-muted);
  }
</style>
