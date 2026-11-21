<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import Modal from './Modal.svelte';
  import FeedIcon from './FeedIcon.svelte';
  import { api, errorText } from '../lib/api';
  import { store } from '../lib/store.svelte';
  import { relativeAgo } from '../lib/util';
  import type { Feed, FeedActivity, FeedPatch } from '../lib/types';

  /** A feed that has posted nothing for this long is "quiet". */
  const QUIET_DAYS = 180;

  let activity = $state<Map<number, FeedActivity> | null>(null);
  let error = $state<string | null>(null);
  /** The feed whose Unsubscribe button was clicked once. */
  let confirming = $state<number | null>(null);

  async function load() {
    try {
      activity = new Map((await api.feedActivity()).map((a) => [a.feedId, a]));
    } catch (e) {
      error = errorText(e);
    }
  }
  load();

  const now = $derived(Math.floor(store.now / 1000));
  const failing = $derived(store.tree.feeds.filter((f) => !f.paused && f.lastError));
  const quiet = $derived(
    store.tree.feeds.filter((f) => {
      if (f.paused || f.lastError || !activity) return false;
      const newest = activity.get(f.id)?.newest ?? null;
      return newest == null || now - newest > QUIET_DAYS * 86_400;
    }),
  );
  const paused = $derived(store.tree.feeds.filter((f) => f.paused));
  const healthy = $derived(store.tree.feeds.length - failing.length - quiet.length - paused.length);

  const ago = (t: number | null) => (t ? relativeAgo(t * 1000, store.now) : null);

  function patchOf(f: Feed, paused: boolean): FeedPatch {
    return {
      customTitle: f.customTitle,
      url: f.url,
      folderId: f.folderId,
      updateInterval: f.updateInterval,
      retentionMode: f.retentionMode,
      retentionValue: f.retentionValue,
      fullText: f.fullText,
      paused,
      typeOverride: f.typeOverride,
    };
  }

  async function setPaused(f: Feed, value: boolean) {
    try {
      await api.updateFeed(f.id, patchOf(f, value));
      await store.reloadTree();
    } catch (e) {
      store.error(e);
    }
  }

  async function unsubscribe(f: Feed) {
    if (confirming !== f.id) {
      confirming = f.id;
      return;
    }
    confirming = null;
    try {
      await api.deleteFeed(f.id);
      if (store.scope.kind === 'feed' && store.scope.id === f.id) store.selectScope({ kind: 'all' });
      await store.reloadTree();
      await store.loadArticles();
      await load();
    } catch (e) {
      store.error(e);
    }
  }

  async function retry(feeds: Feed[]) {
    await store.refresh({ feedIds: feeds.map((f) => f.id) });
  }
</script>

{#snippet row(f: Feed, detail: string, kind: 'failing' | 'quiet' | 'paused')}
  <div class="feed-row">
    <FeedIcon feed={f} size={16} />
    <div class="text">
      <span class="name">{f.title}</span>
      <span class="detail" class:error={kind === 'failing'}>{detail}</span>
    </div>
    <div class="actions">
      {#if kind === 'failing'}
        <button class="btn small ghost" onclick={() => retry([f])} disabled={!!store.progress}>Retry</button>
      {/if}
      <button class="btn small ghost" onclick={() => (store.dialog = { kind: 'feed', feedId: f.id })}>Properties</button>
      {#if kind === 'paused'}
        <button class="btn small ghost" onclick={() => setPaused(f, false)}>Resume</button>
      {:else}
        <button class="btn small ghost" onclick={() => setPaused(f, true)}>Pause</button>
      {/if}
      <button class="btn small" class:danger={confirming === f.id} onclick={() => unsubscribe(f)} onblur={() => confirming === f.id && (confirming = null)}>
        {confirming === f.id ? 'Delete feed and articles' : 'Unsubscribe'}
      </button>
    </div>
  </div>
{/snippet}

<Modal title="Feed health" width={640}>
  {#if error}
    <p class="hint error">{error}</p>
  {:else if !activity}
    <div class="state"><span class="spinner"></span></div>
  {:else}
    <p class="summary">
      <Icon name="check-check" size={14} />
      {healthy} of {store.tree.feeds.length} feed{store.tree.feeds.length === 1 ? '' : 's'} work and posted in the last {QUIET_DAYS / 30} months.
    </p>

    {#if failing.length}
      <section>
        <div class="section-head">
          <h4>Not working ({failing.length})</h4>
          <button class="btn small" onclick={() => retry(failing)} disabled={!!store.progress}><Icon name="refresh" size={13} />Retry all</button>
        </div>
        {#each failing as f (f.id)}
          {@render row(f, `${f.lastError} · ${f.lastSuccess ? `last worked ${ago(f.lastSuccess)}` : 'never worked'}`, 'failing')}
        {/each}
      </section>
    {/if}

    {#if quiet.length}
      <section>
        <h4>Quiet for {QUIET_DAYS / 30} months or more ({quiet.length})</h4>
        {#each quiet as f (f.id)}
          {@const newest = activity.get(f.id)?.newest ?? null}
          {@render row(f, newest ? `Last article ${ago(newest)}` : 'No articles yet', 'quiet')}
        {/each}
      </section>
    {/if}

    {#if paused.length}
      <section>
        <h4>Paused ({paused.length})</h4>
        {#each paused as f (f.id)}
          {@render row(f, f.lastSuccess ? `Last updated ${ago(f.lastSuccess)}` : 'Not updated yet', 'paused')}
        {/each}
      </section>
    {/if}
  {/if}
  {#snippet footer()}
    <button class="btn primary" type="button" onclick={() => (store.dialog = null)}>Done</button>
  {/snippet}
</Modal>

<style>
  .summary {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 14px;
    color: var(--text-2);
    font-size: 13px;
  }
  section + section {
    margin-top: 18px;
  }
  .section-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h4 {
    margin: 0 0 6px;
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-3);
  }
  .feed-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 0;
    border-top: 1px solid var(--border);
  }
  .text {
    display: grid;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }
  .name {
    font-size: 13px;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .detail {
    font-size: 12px;
    color: var(--text-3);
    overflow-wrap: anywhere;
  }
  .detail.error,
  .error {
    color: var(--danger, #d14);
  }
  .actions {
    display: flex;
    gap: 4px;
    flex: none;
  }
  .state {
    display: grid;
    place-items: center;
    padding: 30px;
  }
</style>
