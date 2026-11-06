<script lang="ts">
  import { fly } from 'svelte/transition';
  import Icon from '../lib/Icon.svelte';
  import { store } from '../lib/store.svelte';
</script>

<div class="toasts" aria-live="polite">
  {#each store.toasts as toast (toast.id)}
    <div class="toast {toast.kind}" transition:fly={{ y: 12, duration: 220 }}>
      <span class="icon"><Icon name={toast.kind === 'error' ? 'alert' : toast.kind === 'success' ? 'check' : 'info'} size={15} /></span>
      <span class="text selectable">{toast.text}</span>
      <button class="close" aria-label="Dismiss" onclick={() => store.dismissToast(toast.id)}><Icon name="x" size={12} /></button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    left: 50%;
    bottom: 20px;
    transform: translateX(-50%);
    z-index: 1100;
    display: grid;
    gap: 8px;
    justify-items: center;
    pointer-events: none;
  }
  .toast {
    pointer-events: auto;
    display: flex;
    align-items: center;
    gap: 10px;
    max-width: min(560px, calc(100vw - 40px));
    padding: 9px 10px 9px 12px;
    border-radius: 12px;
    background: var(--bg-elevated);
    box-shadow: var(--shadow-lg);
    font-size: 13px;
    color: var(--text);
  }
  .icon {
    display: grid;
    color: var(--accent);
  }
  .toast.error .icon {
    color: var(--danger);
  }
  .toast.success .icon {
    color: var(--success);
  }
  .text {
    line-height: 1.4;
  }
  .close {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--text-3);
    flex: none;
  }
  .close:hover {
    background: var(--bg-hover);
    color: var(--text);
  }
</style>
