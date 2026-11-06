<script lang="ts">
  import { untrack } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import ArticleBody from './ArticleBody.svelte';
  import { api, errorText } from '../lib/api';
  import { store } from '../lib/store.svelte';
  import type { ReaderPage, Tab } from '../lib/types';
  import { copyText, hostOf } from '../lib/util';

  let { tab, active }: { tab: Extract<Tab, { kind: 'web' }>; active: boolean } = $props();

  let page = $state<ReaderPage | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let imagesOverride = $state<boolean | null>(null);
  let started = false;

  const loadImages = $derived(imagesOverride ?? store.settings.loadImages);
  const host = $derived(hostOf(tab.url));

  async function load(reload = false) {
    loading = true;
    error = null;
    try {
      page = await api.readUrl(tab.url, loadImages, reload);
      store.setTabTitle(tab.id, page.title || host);
    } catch (e) {
      error = errorText(e);
      store.setTabTitle(tab.id, host);
    } finally {
      loading = false;
    }
  }

  // Background tabs load when they are first shown.
  $effect(() => {
    if (active && !started) {
      started = true;
      untrack(() => load());
    }
  });

  async function toggleImages() {
    imagesOverride = !loadImages;
    await load();
  }

  let lastSeq = store.command.seq;
  $effect(() => {
    const cmd = store.command;
    if (cmd.seq === lastSeq) return;
    lastSeq = cmd.seq;
    if (!active) return;
    untrack(() => {
      if (cmd.name === 'browser') store.openExternal(tab.url);
      else if (cmd.name === 'images') toggleImages();
      else if (cmd.name === 'copy-link') copyText(tab.url).then(() => store.toast('Link copied'));
    });
  });
</script>

<div class="article-frame">
  <div class="article-toolbar">
    <div class="group">
      <button class="icon-btn" title="Reload" onclick={() => load(true)} disabled={loading}>
        <Icon name="refresh" size={15} />
      </button>
      <button class="icon-btn" class:active={loadImages} title={loadImages ? 'Hide images (I)' : 'Load images (I)'} onclick={toggleImages}>
        <Icon name={loadImages ? 'image' : 'image-off'} size={16} />
      </button>
      <span class="sep"></span>
      <span class="reader-pill"><Icon name="shield" size={13} /> Reader mode · no scripts, no cookies</span>
    </div>
    <div class="group">
      <button class="icon-btn" title="Copy link" onclick={() => copyText(tab.url).then(() => store.toast('Link copied'))}>
        <Icon name="link" size={15} />
      </button>
      <button class="btn small" onclick={() => store.openExternal(tab.url)}>
        <Icon name="globe" size={13} />Open in browser
      </button>
    </div>
  </div>

  <div class="article-scroll">
    {#if loading && !page}
      <div class="article-state">
        <span class="spinner"></span>
        <p class="loading-host">Loading {host}…</p>
      </div>
    {:else if error}
      <div class="article-state">
        <div class="error-icon"><Icon name="book" size={24} stroke={1.5} /></div>
        <h3>This page can’t be shown in reader mode</h3>
        <p>{error}</p>
        <div class="actions">
          <button class="btn primary" onclick={() => store.openExternal(tab.url)}><Icon name="globe" size={14} />Open in browser</button>
          <button class="btn" onclick={() => load(true)}>Try again</button>
        </div>
        <p class="url selectable">{tab.url}</p>
      </div>
    {:else if page}
      <article class="article wide">
        <header class="article-head">
          <div class="kicker">
            <Icon name="globe" size={14} />
            <span class="site">{page.siteName || host}</span>
            {#if page.published}<span class="dot-sep">·</span><time>{new Date(page.published).toLocaleDateString(undefined, { dateStyle: 'long' })}</time>{/if}
          </div>
          <h1 class="article-title" dir="auto">{page.title}</h1>
          {#if page.byline}<div class="byline" dir="auto">{page.byline}</div>{/if}
        </header>
        {#if page.blockedImages > 0}
          <div class="notice">
            <Icon name="shield" size={16} />
            <span>{page.blockedImages} image{page.blockedImages === 1 ? ' is' : 's are'} hidden for privacy.</span>
            <button class="btn small" onclick={toggleImages}>Load images</button>
          </div>
        {/if}
        <ArticleBody html={page.html} dir={page.dir === 'rtl' || page.dir === 'ltr' ? page.dir : 'auto'} />
        <footer class="article-foot">
          <button class="btn" onclick={() => store.openExternal(tab.url)}><Icon name="globe" size={15} />Open {host}</button>
        </footer>
      </article>
    {/if}
  </div>
</div>

<style>
  .reader-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 11.5px;
    color: var(--text-3);
    white-space: nowrap;
    overflow: hidden;
  }
  .site {
    font-weight: 600;
    color: var(--accent);
  }
  .loading-host {
    font-size: 12.5px;
  }
  .error-icon {
    display: grid;
    place-items: center;
    width: 56px;
    height: 56px;
    border-radius: 16px;
    background: var(--bg-muted);
    margin-bottom: 4px;
  }
  h3 {
    margin: 0;
    color: var(--text);
    font-size: 15px;
  }
  .actions {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }
  .url {
    font-size: 11.5px;
    word-break: break-all;
    max-width: 480px;
    margin-top: 6px !important;
  }
</style>
