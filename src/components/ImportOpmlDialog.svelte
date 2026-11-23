<script lang="ts">
  import { onDestroy } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import Modal from './Modal.svelte';
  import { api, errorText } from '../lib/api';
  import { store } from '../lib/store.svelte';
  import type { ImportLayout, ImportPreview } from '../lib/types';

  let { preview }: { preview: ImportPreview } = $props();

  let layout = $state<ImportLayout>('keep');
  let busy = $state(false);
  let done = false;
  let error = $state<string | null>(null);

  const count = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString()} ${n === 1 ? one : many}`;
  const newFolders = $derived(layout === 'keep' ? preview.newFoldersKeep : preview.newFoldersFile);
  const moving = $derived(layout === 'file' ? preview.moveCount : 0);
  const action = $derived.by(() => {
    const add = preview.newFeeds ? `Add ${count(preview.newFeeds, 'feed')}` : '';
    if (add && moving) return `${add}, move ${moving.toLocaleString()}`;
    if (moving) return `Move ${count(moving, 'feed')}`;
    return add || 'Nothing to import';
  });

  // The backend keeps the file's feeds until the import; forget them if it never runs.
  onDestroy(() => {
    if (!done) api.cancelOpmlImport().catch(() => {});
  });

  async function run() {
    busy = true;
    error = null;
    try {
      done = await store.finishOpmlImport(layout);
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title="Import subscriptions" width={520}>
  <div class="file">
    <Icon name="download" size={16} />
    <div class="name">{preview.fileName}</div>
  </div>

  <ul class="facts">
    <li>{count(preview.inFile, 'feed')} in the file</li>
    <li>
      <strong>{count(preview.newFeeds, 'new feed')}</strong>{#if preview.newTitles.length}<span class="muted"
          >: {preview.newTitles.join(', ')}{preview.newFeeds > preview.newTitles.length ? ', …' : ''}</span
        >{/if}
    </li>
    <li>{count(preview.subscribed, 'feed')} you already follow, left out so nothing is added twice</li>
    {#if preview.repeated}<li>{count(preview.repeated, 'feed')} listed more than once in the file</li>{/if}
    {#if preview.invalid}<li>{count(preview.invalid, 'entry', 'entries')} without a usable feed address, left out</li>{/if}
  </ul>

  {#if preview.moveCount}
    <div class="section">
      <p class="lead">
        The file puts {count(preview.moveCount, 'feed')} you already follow in {preview.moveCount === 1 ? 'a different folder' : 'different folders'}:
      </p>
      <ul class="moves">
        {#each preview.moves as m, i (i)}
          <li><span class="title">{m.title}</span><span class="path">{m.from}</span><Icon name="chevron-right" size={11} /><span class="path">{m.to}</span></li>
        {/each}
        {#if preview.moveCount > preview.moves.length}
          <li class="muted">and {count(preview.moveCount - preview.moves.length, 'more feed')}</li>
        {/if}
      </ul>

      <div class="choices" role="radiogroup" aria-label="Folders">
        <label class="choice" class:on={layout === 'keep'}>
          <input type="radio" name="layout" value="keep" bind:group={layout} />
          <span>
            <strong>Keep my folders</strong>
            <span class="hint">
              Feeds you follow stay where they are. New feeds go into the file's folders; folders you don't have yet are
              added at the end{preview.newFoldersKeep ? ` (${count(preview.newFoldersKeep, 'new folder')})` : ''}.
            </span>
          </span>
        </label>
        <label class="choice" class:on={layout === 'file'}>
          <input type="radio" name="layout" value="file" bind:group={layout} />
          <span>
            <strong>Use the file's folders</strong>
            <span class="hint">
              Also moves the {count(preview.moveCount, 'feed')} above to the folders the file puts them in{preview.newFoldersFile
                ? `, with ${count(preview.newFoldersFile, 'new folder')}`
                : ''}. Folders left empty are kept, with their filter rules.
            </span>
          </span>
        </label>
      </div>
    </div>
  {:else if newFolders}
    <p class="note">New feeds go into the file's folders. {count(newFolders, 'folder')} you don't have yet {newFolders === 1 ? 'is' : 'are'} added at the end.</p>
  {/if}

  {#if error}<p class="note error">{error}</p>{/if}

  {#snippet footer()}
    <button class="btn" type="button" onclick={() => (store.dialog = null)} disabled={busy}>Cancel</button>
    <button class="btn primary" type="button" onclick={run} disabled={busy || action === 'Nothing to import'}>
      {#if busy}<span class="spinner"></span>Importing…{:else}{action}{/if}
    </button>
  {/snippet}
</Modal>

<style>
  .file {
    display: flex;
    gap: 10px;
    align-items: center;
    color: var(--text-2);
  }
  .name {
    font-size: 13px;
    font-weight: 600;
    color: var(--text);
    overflow-wrap: anywhere;
  }
  .facts {
    margin: 14px 0;
    padding-left: 20px;
    font-size: 13px;
    line-height: 1.7;
  }
  .muted {
    color: var(--text-3);
  }
  .facts .muted {
    overflow-wrap: anywhere;
  }
  .section {
    display: grid;
    gap: 10px;
  }
  .lead,
  .note {
    margin: 0;
    font-size: 13px;
    line-height: 1.45;
  }
  .note {
    color: var(--text-2);
  }
  .error {
    margin-top: 12px;
    color: var(--danger, #d14);
  }
  .moves {
    display: grid;
    gap: 4px;
    margin: 0;
    padding: 8px 12px;
    list-style: none;
    border-radius: 9px;
    background: var(--bg-muted);
    font-size: 12px;
    max-height: 168px;
    overflow-y: auto;
  }
  .moves li {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    color: var(--text-2);
  }
  .moves .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    color: var(--text);
    font-weight: 500;
  }
  .moves .path {
    max-width: 34%;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .choices {
    display: grid;
    gap: 6px;
  }
  .choice {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 10px 12px;
    border-radius: 10px;
    border: 1px solid var(--border-strong);
    transition: border-color 0.15s, background 0.15s;
  }
  .choice.on {
    border-color: var(--accent);
    background: var(--accent-softer);
  }
  .choice input {
    margin-top: 2px;
    accent-color: var(--accent);
  }
  .choice > span {
    display: grid;
    gap: 2px;
  }
  .choice strong {
    font-size: 13px;
  }
  .choice .hint {
    font-size: 12px;
    line-height: 1.4;
    color: var(--text-2);
  }
</style>
