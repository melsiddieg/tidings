<script lang="ts">
  import { untrack } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import Modal from './Modal.svelte';
  import FeedIcon from './FeedIcon.svelte';
  import { api, errorText } from '../lib/api';
  import { store } from '../lib/store.svelte';
  import { folderOptions, hostOf, relativeAgo } from '../lib/util';

  let { feedId }: { feedId: number } = $props();

  const feed = untrack(() => store.feedById.get(feedId));

  let title = $state(feed?.customTitle ?? '');
  let url = $state(feed?.url ?? '');
  let folderId = $state<number | null>(feed?.folderId ?? null);
  // "default" | minutes as string
  let interval = $state(feed?.updateInterval == null ? 'default' : String(feed.updateInterval));
  let retention = $state<string>(feed?.retentionMode ?? 'default');
  let retentionValue = $state<number>(
    feed?.retentionValue ?? (feed?.retentionMode === 'days' ? store.settings.retentionDays : store.settings.retentionCount),
  );
  let fullText = $state(feed?.fullText ?? false);
  let paused = $state(feed?.paused ?? false);
  let saving = $state(false);
  let error = $state<string | null>(null);

  const folders = $derived(folderOptions(store.root));
  const defaultInterval = $derived(
    store.settings.refreshInterval === 0 ? 'manual' : `every ${formatMinutes(store.settings.refreshInterval)}`,
  );

  const INTERVALS = [15, 30, 60, 120, 240, 720, 1440];

  function formatMinutes(m: number): string {
    if (m < 60) return `${m} minutes`;
    if (m === 60) return 'hour';
    if (m < 1440) return `${m / 60} hours`;
    return 'day';
  }

  async function save(e: SubmitEvent) {
    e.preventDefault();
    if (!feed) return;
    saving = true;
    error = null;
    try {
      await api.updateFeed(feed.id, {
        customTitle: title.trim() || null,
        url: url.trim(),
        folderId,
        updateInterval: interval === 'default' ? null : Number(interval),
        retentionMode: retention === 'default' ? null : retention,
        retentionValue: retention === 'count' || retention === 'days' ? Math.max(1, Number(retentionValue) || 1) : null,
        fullText,
        paused,
      });
      await store.reloadTree();
      store.dialog = null;
    } catch (err) {
      error = errorText(err);
    } finally {
      saving = false;
    }
  }
</script>

<Modal title="Feed properties" width={540}>
  {#if feed}
    <form id="feed-props" class="form" onsubmit={save}>
      <div class="summary">
        <FeedIcon {feed} size={36} />
        <div class="summary-text">
          <strong>{feed.title}</strong>
          <span>
            {feed.total} articles · {feed.unread} unread
            {#if feed.lastSuccess} · updated {relativeAgo(feed.lastSuccess * 1000, store.now)}{/if}
          </span>
          {#if feed.siteUrl}
            <button type="button" class="link" onclick={() => store.openExternal(feed.siteUrl)}>{hostOf(feed.siteUrl)} <Icon name="external" size={11} /></button>
          {/if}
        </div>
      </div>

      {#if feed.lastError}
        <div class="error"><Icon name="alert" size={14} /><span class="selectable">Last update failed: {feed.lastError}</span></div>
      {/if}

      <div class="grid2">
        <div class="field">
          <label for="fp-title">Name</label>
          <input id="fp-title" class="input" bind:value={title} placeholder={feed.feedTitle || feed.url} />
        </div>
        <div class="field">
          <label for="fp-folder">Folder</label>
          <select id="fp-folder" class="input" bind:value={folderId}>
            <option value={null}>No folder</option>
            {#each folders as f (f.id)}
              <option value={f.id}>{'  '.repeat(f.depth)}{f.title}</option>
            {/each}
          </select>
        </div>
      </div>

      <div class="field">
        <label for="fp-url">Feed address</label>
        <input id="fp-url" class="input mono" bind:value={url} spellcheck="false" autocomplete="off" />
      </div>

      <div class="grid2">
        <div class="field">
          <label for="fp-interval">Check for updates</label>
          <select id="fp-interval" class="input" bind:value={interval}>
            <option value="default">Default ({defaultInterval})</option>
            {#each INTERVALS as m (m)}
              <option value={String(m)}>Every {formatMinutes(m)}</option>
            {/each}
            <option value="0">Manually only</option>
          </select>
        </div>
        <div class="field">
          <label for="fp-archive">Archive</label>
          <div class="archive-row">
            <select id="fp-archive" class="input" bind:value={retention}>
              <option value="default">Default</option>
              <option value="all">Keep everything</option>
              <option value="count">Keep newest…</option>
              <option value="days">Keep for days…</option>
            </select>
            {#if retention === 'count' || retention === 'days'}
              <input class="input num" type="number" min="1" bind:value={retentionValue} aria-label={retention === 'count' ? 'Articles' : 'Days'} />
            {/if}
          </div>
        </div>
      </div>

      <div class="toggles">
        <label class="toggle">
          <span>
            <strong>Always load the full article</strong>
            <span class="hint">For feeds that only send a summary. Tidings downloads the page and shows it in reader mode.</span>
          </span>
          <button type="button" class="switch" class:on={fullText} aria-pressed={fullText} onclick={() => (fullText = !fullText)} aria-label="Always load the full article"></button>
        </label>
        <label class="toggle">
          <span>
            <strong>Pause automatic updates</strong>
            <span class="hint">The feed is only refreshed when you ask for it.</span>
          </span>
          <button type="button" class="switch" class:on={paused} aria-pressed={paused} onclick={() => (paused = !paused)} aria-label="Pause automatic updates"></button>
        </label>
      </div>

      {#if error}
        <div class="error"><Icon name="alert" size={14} /><span class="selectable">{error}</span></div>
      {/if}
    </form>
  {/if}

  {#snippet footer()}
    {#if feed}
      <button class="btn ghost danger-text" type="button" onclick={() => store.deleteFeed(feed)}>
        <Icon name="trash" size={14} />Unsubscribe
      </button>
    {/if}
    <span class="grow"></span>
    <button class="btn" type="button" onclick={() => (store.dialog = null)}>Cancel</button>
    <button class="btn primary" type="submit" form="feed-props" disabled={saving}>Save</button>
  {/snippet}
</Modal>

<style>
  .form {
    display: grid;
    gap: 16px;
  }
  .summary {
    display: flex;
    gap: 14px;
    align-items: center;
    padding: 12px 14px;
    border-radius: 12px;
    background: var(--bg-muted);
  }
  .summary :global(.feed-icon) {
    border-radius: 9px;
  }
  .summary-text {
    display: grid;
    gap: 1px;
    min-width: 0;
  }
  .summary-text strong {
    font-size: 14px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .summary-text > span {
    font-size: 12px;
    color: var(--text-3);
  }
  .summary-text .link {
    justify-self: start;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: 12px;
  }
  .grid2 {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }
  .mono {
    font-family: var(--font-mono);
    font-size: 12px;
  }
  .archive-row {
    display: flex;
    gap: 6px;
  }
  .num {
    width: 86px;
    flex: none;
  }
  .toggles {
    display: grid;
    gap: 2px;
    border-radius: 12px;
    border: 1px solid var(--border);
    overflow: hidden;
  }
  .toggle {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 12px 14px;
  }
  .toggle + .toggle {
    border-top: 1px solid var(--border);
  }
  .toggle > span {
    flex: 1;
    display: grid;
    gap: 2px;
  }
  .toggle strong {
    font-size: 13px;
    font-weight: 500;
  }
  .error {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    padding: 10px 12px;
    border-radius: 10px;
    color: var(--danger);
    background: color-mix(in srgb, var(--danger) 9%, transparent);
    font-size: 12.5px;
    line-height: 1.45;
  }
  .grow {
    flex: 1;
  }
  .danger-text {
    color: var(--danger);
  }
</style>
