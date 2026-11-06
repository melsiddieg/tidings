<script lang="ts">
  import Modal from './Modal.svelte';
  import { store } from '../lib/store.svelte';
  import type { Dialog } from '../lib/types';

  let { dialog }: { dialog: Extract<Dialog, { kind: 'confirm' }> } = $props();
  let busy = $state(false);

  async function confirm() {
    busy = true;
    try {
      await dialog.onConfirm();
    } finally {
      busy = false;
      // Close unless the action opened a different dialog.
      if (store.dialog === dialog) store.dialog = null;
    }
  }
</script>

<Modal width={420} padded={false}>
  <div class="confirm">
    <h2>{dialog.title}</h2>
    <p>{dialog.message}</p>
  </div>
  {#snippet footer()}
    <button class="btn" onclick={() => (store.dialog = null)}>Cancel</button>
    <button class="btn {dialog.danger ? 'danger' : 'primary'}" onclick={confirm} disabled={busy} data-autofocus>
      {dialog.confirmLabel}
    </button>
  {/snippet}
</Modal>

<style>
  .confirm {
    padding: 22px 24px 20px;
  }
  h2 {
    margin: 0 0 8px;
    font-size: 15px;
  }
  p {
    margin: 0;
    color: var(--text-2);
    line-height: 1.5;
  }
</style>
