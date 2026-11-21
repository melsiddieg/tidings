<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import Modal from './Modal.svelte';
  import { errorText } from '../lib/api';
  import { store } from '../lib/store.svelte';
  import type { BackupSummary } from '../lib/types';

  let { summary }: { summary: BackupSummary } = $props();

  let busy = $state(false);
  let error = $state<string | null>(null);

  const count = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString()} ${n === 1 ? one : many}`;
  const made = $derived(
    summary.createdAt
      ? new Date(summary.createdAt * 1000).toLocaleString(undefined, { dateStyle: 'long', timeStyle: 'short' })
      : null,
  );
  const proxy = $derived.by(() => {
    if (summary.proxyNow && summary.proxyAfter === summary.proxyNow) return `Tidings keeps using the proxy ${summary.proxyNow}.`;
    if (summary.proxyAfter) return `Tidings will connect through the backup's proxy, ${summary.proxyAfter}.`;
    return 'No proxy, as now.';
  });

  async function restore() {
    busy = true;
    error = null;
    try {
      // Reloads the window when it is done.
      await store.restoreBackup();
    } catch (e) {
      error = errorText(e);
      busy = false;
    }
  }
</script>

<Modal title="Restore backup" width={500}>
  <div class="file">
    <Icon name="archive" size={16} />
    <div>
      <div class="name">{summary.fileName}</div>
      <div class="hint">
        {#if made}Made {made}{summary.version ? ` by Tidings ${summary.version}` : ''}.{:else}A copy of a Tidings database.{/if}
      </div>
    </div>
  </div>

  <ul class="facts">
    <li>{count(summary.feeds, 'feed')} in {count(summary.folders, 'folder')}</li>
    <li>{count(summary.articles, 'article')}: {summary.unread.toLocaleString()} unread, {summary.starred.toLocaleString()} starred</li>
    <li>{count(summary.rules, 'filter rule')} and {count(summary.searches, 'saved search', 'saved searches')}</li>
  </ul>

  <div class="notes">
    {#if summary.localFeeds.length}
      <p>
        <Icon name="shield" size={13} />
        <span>These addresses on your local network keep the permission you gave them: {summary.localFeeds.join(', ')}.</span>
      </p>
    {/if}
    <p>
      <Icon name="globe" size={13} />
      <span>
        {proxy} The other settings come from the backup. A proxy, “HTTPS only” and tracker removal that are on now stay on, and
        the video player stays as set on this computer.
      </span>
    </p>
    {#if summary.offline}
      <p class="warn"><Icon name="alert" size={13} /><span>{summary.offline}</span></p>
    {/if}
  </div>

  <p class="replace">
    Restoring replaces every feed, article, rule, saved search and setting in Tidings now. Tidings first copies them to
    <code>tidings-before-restore.db</code> in its data folder; restore that file to go back.
  </p>
  {#if error}<p class="hint error">{error}</p>{/if}

  {#snippet footer()}
    <button class="btn" type="button" onclick={() => (store.dialog = null)} disabled={busy}>Cancel</button>
    <button class="btn danger" type="button" onclick={restore} disabled={busy}>
      {#if busy}<span class="spinner"></span>Restoring…{:else}Replace and restore{/if}
    </button>
  {/snippet}
</Modal>

<style>
  .file {
    display: flex;
    gap: 10px;
    align-items: flex-start;
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
  .notes p {
    display: flex;
    gap: 8px;
    margin: 0 0 8px;
    font-size: 12.5px;
    line-height: 1.45;
    color: var(--text-2);
  }
  .notes :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  .notes span {
    overflow-wrap: anywhere;
  }
  .warn,
  .error {
    color: var(--danger, #d14);
  }
  .notes p.warn {
    color: var(--danger, #d14);
  }
  .replace {
    margin: 14px 0 0;
    font-size: 12.5px;
    line-height: 1.45;
    color: var(--text-2);
  }
</style>
