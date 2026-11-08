<script lang="ts">
  import { untrack } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import FeedIcon from './FeedIcon.svelte';
  import ArticleBody from './ArticleBody.svelte';
  import VideoCard from './VideoCard.svelte';
  import { api, errorText } from '../lib/api';
  import { player, isAudio } from '../lib/player.svelte';
  import { store } from '../lib/store.svelte';
  import type { ArticleDetail } from '../lib/types';
  import { copyText, enclosureLabel, hostOf, isVideoFile, longTime, videoSource } from '../lib/util';

  let {
    articleId,
    mode = 'preview',
    active = false,
    tabId = null,
  }: { articleId: number; mode?: 'preview' | 'tab'; active?: boolean; tabId?: string | null } = $props();

  let detail = $state<ArticleDetail | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let fullLoading = $state(false);
  let imagesOverride = $state<boolean | null>(null);
  let scroller: HTMLDivElement | undefined = $state();
  let token = 0;

  const feed = $derived(detail ? store.feedById.get(detail.feedId) : undefined);
  const loadImages = $derived(imagesOverride ?? store.settings.loadImages);
  const read = $derived(detail ? store.isRead(detail) : false);
  const starred = $derived(detail ? store.isStarred(detail) : false);
  // The article's video: a PeerTube page, an attached video file, or a video site link.
  const video = $derived(detail ? videoSource(detail.url, detail.enclosureUrl, detail.enclosureType) : null);
  // What mpv or IINA gets (W, toolbar).
  const videoUrl = $derived(video?.url ?? null);
  const thumb = $derived(detail ? store.articles.find((a) => a.id === detail?.id)?.thumb || null : null);
  const playerName = $derived(store.videoPlayer?.name ?? 'video player');

  async function load(id: number) {
    const t = ++token;
    loading = !detail || detail.id !== id;
    error = null;
    try {
      const d = await api.article(id, loadImages, false);
      if (t !== token) return;
      detail = d;
      if (tabId) store.setTabTitle(tabId, d.title);
      if (store.feedById.get(d.feedId)?.fullText && d.url) toggleFullText(true);
    } catch (e) {
      if (t === token) error = errorText(e);
    } finally {
      if (t === token) loading = false;
    }
  }

  $effect(() => {
    const id = articleId;
    untrack(() => {
      imagesOverride = null;
      fullLoading = false;
      load(id);
      scroller?.scrollTo({ top: 0 });
    });
  });

  let lastImageSetting = store.settings.loadImages;
  $effect(() => {
    const value = store.settings.loadImages;
    if (value === lastImageSetting) return;
    lastImageSetting = value;
    untrack(() => {
      if (detail && imagesOverride === null) {
        api.article(detail.id, value, detail.showingFullText).then((d) => {
          if (d.id === articleId) detail = d;
        });
      }
    });
  });

  async function toggleFullText(force = false) {
    if (!detail) return;
    const id = detail.id;
    if (detail.showingFullText && !force) {
      detail = await api.article(id, loadImages, false);
      return;
    }
    if (!detail.url) return store.toast('This article has no link to load');
    fullLoading = true;
    try {
      const d = await api.fullText(id, loadImages);
      if (d.id === articleId) detail = d;
    } catch (e) {
      if (!force) store.error(e);
    } finally {
      fullLoading = false;
    }
  }

  async function toggleImages() {
    if (!detail) return;
    imagesOverride = !loadImages;
    try {
      detail = await api.article(detail.id, imagesOverride, detail.showingFullText);
    } catch (e) {
      store.error(e);
    }
  }

  function moreMenu(e: MouseEvent) {
    if (!detail) return;
    const d = detail;
    store.openMenuAt(e.currentTarget as HTMLElement, [
      { label: 'Copy link', icon: 'link', disabled: !d.url, action: () => d.url && copyText(d.url).then(() => store.toast('Link copied')) },
      { label: 'Open in reader tab', icon: 'book', disabled: !d.url, action: () => d.url && store.openWebTab(d.url, false, d.title) },
      { label: `Show articles from ${feed?.title ?? 'this feed'}`, icon: 'rss', action: () => store.selectScope({ kind: 'feed', id: d.feedId }) },
      { separator: true },
      { label: 'Delete article', icon: 'trash', danger: true, action: () => store.deleteArticle(d.id) },
    ]);
  }

  // Keyboard commands from App.svelte, only for the view the user is looking at.
  let lastSeq = store.command.seq;
  $effect(() => {
    const cmd = store.command;
    if (cmd.seq === lastSeq) return;
    lastSeq = cmd.seq;
    if (!active || !detail) return;
    untrack(() => run(cmd.name));
  });

  function run(name: string) {
    if (!detail) return;
    switch (name) {
      case 'star':
        store.setStarred(detail.id, !starred);
        break;
      case 'read':
        store.markRead([detail.id], !read);
        break;
      case 'full-text':
        toggleFullText();
        break;
      case 'images':
        toggleImages();
        break;
      case 'browser':
        store.openExternal(detail.url);
        break;
      case 'watch':
        if (videoUrl) store.watchVideo(videoUrl);
        else store.toast('This article has no video');
        break;
      case 'copy-link':
        if (detail.url) copyText(detail.url).then(() => store.toast('Link copied'));
        break;
      case 'delete':
        store.deleteArticle(detail.id);
        break;
    }
  }
</script>

<div class="article-frame">
  {#if detail}
    <div class="article-toolbar">
      <div class="group">
        <button class="icon-btn" class:active={starred} title={starred ? 'Remove star (S)' : 'Star (S)'} onclick={() => detail && store.setStarred(detail.id, !starred)}>
          <Icon name="star" size={16} fill={starred} />
        </button>
        <button class="icon-btn" title={read ? 'Mark as unread (M)' : 'Mark as read (M)'} onclick={() => detail && store.markRead([detail.id], !read)}>
          <Icon name={read ? 'circle' : 'dot'} size={16} />
        </button>
        <span class="sep"></span>
        <button class="tool" class:on={detail.showingFullText} title="Load the full article from the website (F)" onclick={() => toggleFullText()} disabled={!detail.url}>
          {#if fullLoading}<span class="spinner"></span>{:else}<Icon name="book" size={15} />{/if}
          <span>Full text</span>
        </button>
        <button class="icon-btn" class:active={loadImages} title={loadImages ? 'Hide images (I)' : 'Load images (I)'} onclick={toggleImages}>
          <Icon name={loadImages ? 'image' : 'image-off'} size={16} />
        </button>
      </div>
      <div class="group">
        {#if mode === 'preview'}
          <button class="icon-btn" title="Open in tab (O)" onclick={() => detail && store.openArticleTab(detail.id)}>
            <Icon name="external" size={15} />
          </button>
        {/if}
        {#if videoUrl}
          <button class="icon-btn" title={`Watch in ${playerName} (W)`} onclick={() => store.watchVideo(videoUrl)}>
            <Icon name="video" size={16} />
          </button>
        {/if}
        <button class="icon-btn" title="Open in browser (V)" onclick={() => store.openExternal(detail?.url)} disabled={!detail.url}>
          <Icon name="globe" size={16} />
        </button>
        <button class="icon-btn" title="More" onclick={moreMenu}><Icon name="more" size={16} /></button>
      </div>
    </div>

    <div class="article-scroll" bind:this={scroller}>
      <article class="article" class:wide={mode === 'tab'}>
        <header class="article-head">
          <div class="kicker">
            <FeedIcon {feed} size={16} />
            <button class="feed-link" onclick={() => detail && store.selectScope({ kind: 'feed', id: detail.feedId })}>{feed?.title ?? hostOf(detail.url)}</button>
            <span class="dot-sep">·</span>
            <time>{longTime(detail.published)}</time>
          </div>
          <h1 class="article-title" dir="auto">
            {#if detail.url}
              <a href={detail.url} onclick={(e) => { e.preventDefault(); store.openExternal(detail?.url); }}>{detail.title}</a>
            {:else}
              {detail.title}
            {/if}
          </h1>
          {#if detail.author}<div class="byline" dir="auto">By {detail.author}</div>{/if}
        </header>

        {#if video}
          {#key detail.id}
            <VideoCard source={video} title={detail.title} poster={thumb} />
          {/key}
        {/if}

        {#if detail.enclosureUrl && isAudio(detail.enclosureType, detail.enclosureUrl)}
          {@const d = detail}
          {@const current = player.isCurrent(d.enclosureUrl)}
          <div class="enclosure episode-card" class:current>
            <button
              class="episode-play"
              aria-label={current && player.playing ? 'Pause' : 'Play episode'}
              onclick={() => d.enclosureUrl && player.play({ articleId: d.id, feedId: d.feedId, title: d.title, url: d.enclosureUrl, type: d.enclosureType })}
            >
              <Icon name={current && player.playing ? 'pause-fill' : 'play'} size={18} stroke={0} />
            </button>
            <span class="enclosure-text">
              <strong>{current ? (player.playing ? 'Playing' : 'Paused') : 'Play episode'}</strong>
              <span>{hostOf(d.enclosureUrl)} · streamed privately through Tidings</span>
            </span>
            <button class="icon-btn" title="Open the audio file in your default app" onclick={() => store.openExternal(d.enclosureUrl)}>
              <Icon name="external" size={14} />
            </button>
          </div>
        {:else if detail.enclosureUrl && !(video && isVideoFile(detail.enclosureType, detail.enclosureUrl))}
          <button class="enclosure" onclick={() => store.openExternal(detail?.enclosureUrl)}>
            <span class="enclosure-icon"><Icon name="paperclip" size={18} /></span>
            <span class="enclosure-text">
              <strong>{enclosureLabel(detail.enclosureType)}</strong>
              <span>{hostOf(detail.enclosureUrl)} · opens in your default app</span>
            </span>
            <Icon name="external" size={14} />
          </button>
        {/if}

        {#if detail.blockedImages > 0}
          <div class="notice">
            <Icon name="shield" size={16} />
            <span>{detail.blockedImages} image{detail.blockedImages === 1 ? ' is' : 's are'} hidden for privacy.</span>
            <button class="btn small" onclick={toggleImages}>Load images</button>
          </div>
        {/if}

        {#if detail.showingFullText}
          <div class="fulltext-note"><Icon name="book" size={13} /> Full article loaded from {hostOf(detail.url)}</div>
        {/if}

        <ArticleBody html={detail.html} />

        {#if !detail.html.trim()}
          <p class="no-content">This feed only sends a headline.
            {#if detail.url}<button class="link" onclick={() => toggleFullText()}>Load the full article</button>{/if}
          </p>
        {/if}

        <footer class="article-foot">
          {#if detail.url && !detail.showingFullText}
            <button class="btn" onclick={() => toggleFullText()}><Icon name="book" size={15} />Read full article</button>
          {/if}
          {#if detail.url}
            <button class="btn ghost" onclick={() => store.openExternal(detail?.url)}><Icon name="globe" size={15} />Open {hostOf(detail.url)}</button>
          {/if}
        </footer>
      </article>
    </div>
  {:else if loading}
    <div class="article-state"><span class="spinner"></span></div>
  {:else if error}
    <div class="article-state">
      <Icon name="alert" size={22} />
      <p>{error}</p>
    </div>
  {/if}
</div>
