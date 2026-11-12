<script lang="ts">
  import { untrack } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import Modal from './Modal.svelte';
  import { api, errorText } from '../lib/api';
  import { store } from '../lib/store.svelte';
  import type { Candidate, Dialog } from '../lib/types';
  import { folderOptions, hostOf } from '../lib/util';

  let { dialog }: { dialog: Extract<Dialog, { kind: 'add-feed' }> } = $props();

  let url = $state(untrack(() => dialog.url ?? ''));
  let folderId = $state<number | null>(untrack(() => dialog.folderId));
  let busy = $state(false);
  let error = $state<string | null>(null);
  let candidates = $state<Candidate[]>([]);
  let chosen = $state(0);
  // An address on the local network is only contacted, and a feed there only
  // subscribed, after the user says so for that host. The backend refuses with
  // "local-network:<host>" until then.
  const CONSENT = 'local-network:';
  let consent = $state<{ host: string; retry: () => void } | null>(null);
  let allowedHost = $state<string | null>(null);

  function hostname(address: string): string {
    try {
      return new URL(address).hostname;
    } catch {
      return '';
    }
  }
  const allowed = (c: Candidate) => allowedHost !== null && hostname(c.url) === allowedHost;

  /** Shows the consent question if `e` asks for it. */
  function asksConsent(e: unknown, retry: () => void): boolean {
    const text = errorText(e);
    if (!text.startsWith(CONSENT)) return false;
    consent = { host: text.slice(CONSENT.length), retry };
    return true;
  }

  function allow() {
    if (!consent) return;
    const { host, retry } = consent;
    allowedHost = host;
    consent = null;
    retry();
  }

  const folders = $derived(folderOptions(store.root));

  async function find(allowLocal = false) {
    if (!url.trim()) return;
    busy = true;
    error = null;
    consent = null;
    candidates = [];
    try {
      const found = await api.discover(url.trim(), allowLocal);
      // A single feed is subscribed at once, except one on the local network that
      // the user has not allowed: a web page can name any address as its feed.
      if (found.length === 1 && (!found[0].local || allowed(found[0]))) await subscribe(found[0]);
      else {
        candidates = found;
        chosen = 0;
      }
    } catch (e) {
      if (!asksConsent(e, () => find(true))) error = errorText(e);
    } finally {
      busy = false;
    }
  }

  async function subscribe(candidate: Candidate, allowLocal = allowed(candidate)) {
    busy = true;
    error = null;
    consent = null;
    try {
      const title = candidate.title.trim() || undefined;
      const id = await api.addFeed(candidate.url, folderId, title, allowLocal);
      store.watchFeed(id);
      await store.reloadTree();
      store.selectScope({ kind: 'feed', id });
      store.dialog = null;
      store.toast(`Subscribed to ${title ?? hostOf(candidate.url)}`, 'success');
    } catch (e) {
      if (!asksConsent(e, () => subscribe(candidate, true))) error = errorText(e);
    } finally {
      busy = false;
    }
  }

  function onsubmit(e: SubmitEvent) {
    e.preventDefault();
    if (candidates.length > 0) subscribe(candidates[chosen]);
    else find();
  }
</script>

<Modal title="Add a feed" width={500}>
  <form id="add-feed" class="form" {onsubmit}>
    <div class="field">
      <label for="feed-url">Website or feed address</label>
      <div class="url-row">
        <span class="url-icon"><Icon name="link" size={14} /></span>
        <input
          id="feed-url"
          class="input with-icon"
          bind:value={url}
          oninput={() => {
            candidates = [];
            error = null;
            consent = null;
            allowedHost = null;
          }}
          placeholder="example.com or https://example.com/feed.xml"
          spellcheck="false"
          autocomplete="off"
          autocapitalize="off"
        />
      </div>
      <p class="hint">Paste a website and Tidings will find its feed. RSS, Atom and JSON Feed are supported.</p>
    </div>

    {#if candidates.length > 0}
      <div class="field">
        <span class="field-label">{candidates.length === 1 ? 'This site names one feed' : `This site has ${candidates.length} feeds`}</span>
        <div class="candidates" role="radiogroup">
          {#each candidates as c, i (c.url)}
            <label class="candidate" class:on={chosen === i}>
              <input type="radio" name="candidate" value={i} bind:group={chosen} />
              <span class="c-text">
                <strong>{c.title || hostOf(c.url)}</strong>
                <span class="selectable">{c.url}</span>
              </span>
              <span class="kind">{c.local ? 'Local network · ' : ''}{c.kind}</span>
            </label>
          {/each}
        </div>
      </div>
    {/if}

    <div class="field">
      <label for="feed-folder">Folder</label>
      <select id="feed-folder" class="input" bind:value={folderId}>
        <option value={null}>No folder</option>
        {#each folders as f (f.id)}
          <option value={f.id}>{'  '.repeat(f.depth)}{f.title}</option>
        {/each}
      </select>
    </div>

    {#if consent}
      <div class="consent">
        <Icon name="alert" size={16} />
        <span>
          <strong class="selectable">{consent.host}</strong> is on your local network. Tidings would send requests to that
          device, now and at every refresh. Allow this only if you know the address, such as your own NAS or home server.
        </span>
        <button class="btn small" type="button" onclick={allow}>Allow</button>
      </div>
    {/if}
    {#if error}
      <div class="error"><Icon name="alert" size={14} /><span class="selectable">{error}</span></div>
    {/if}
  </form>

  {#snippet footer()}
    <button class="btn" type="button" onclick={() => (store.dialog = null)}>Cancel</button>
    <button class="btn primary" type="submit" form="add-feed" disabled={busy || !url.trim()}>
      {#if busy}<span class="spinner"></span>{/if}
      {candidates.length > 0 ? 'Subscribe' : busy ? 'Looking…' : 'Add feed'}
    </button>
  {/snippet}
</Modal>

<style>
  .consent {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--bg-muted);
    font-size: 13px;
    line-height: 1.45;
  }
  .consent span {
    flex: 1;
  }
  .form {
    display: grid;
    gap: 18px;
  }
  .url-row {
    position: relative;
  }
  .url-icon {
    position: absolute;
    left: 11px;
    top: 50%;
    transform: translateY(-50%);
    color: var(--text-3);
    display: grid;
  }
  .with-icon {
    padding-left: 32px;
  }
  .candidates {
    display: grid;
    gap: 6px;
  }
  .candidate {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    border-radius: 10px;
    border: 1px solid var(--border-strong);
    transition: border-color 0.15s, background 0.15s;
  }
  .candidate.on {
    border-color: var(--accent);
    background: var(--accent-softer);
  }
  .candidate input {
    accent-color: var(--accent);
  }
  .c-text {
    flex: 1;
    display: grid;
    min-width: 0;
  }
  .c-text strong {
    font-size: 13px;
  }
  .c-text span {
    font-size: 11.5px;
    color: var(--text-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .kind {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-2);
    padding: 2px 7px;
    border-radius: 6px;
    background: var(--bg-muted);
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
  .error :global(svg) {
    margin-top: 1px;
  }
</style>
