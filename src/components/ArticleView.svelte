<script lang="ts">
  import { untrack } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import FeedIcon from './FeedIcon.svelte';
  import ArticleBody from './ArticleBody.svelte';
  import VideoCard from './VideoCard.svelte';
  import { api, errorText, on } from '../lib/api';
  import { player, isAudio } from '../lib/player.svelte';
  import { store } from '../lib/store.svelte';
  import type { ArticleDetail, TranslateLang, TranslationBlock, TranslationPart } from '../lib/types';
  import { LANGUAGE_NAMES, possibleTargets, translationTargets } from '../lib/filters';
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
  // The translation shown instead of the article: filled part by part while the
  // local model works (`translating`).
  let shown = $state<{
    title: string | null;
    paragraphs: string[];
    total: number;
    model: string | null;
    cut: boolean;
    lang: TranslateLang;
  } | null>(null);
  let translating = $state(false);
  // Counts up with each translation, so a stopped one leaves the next one alone.
  let translationRun = 0;
  // The languages the Translate button offers at one click, the usual one first,
  // and every language its menu offers.
  const targets = $derived(detail ? translationTargets(detail.language, store.settings.translateTo) : []);
  const possible = $derived(detail ? possibleTargets(detail.language) : []);
  const otherLanguage = $derived(shown ? possible.find((t) => t !== shown?.lang) : undefined);
  let languageButton: HTMLButtonElement | undefined = $state();

  /** The Translate button's menu: translate into either language, and the usual one. */
  function translateMenu() {
    if (!languageButton || !detail) return;
    const usual = store.settings.translateTo;
    store.openMenuAt(languageButton, [
      ...possible.map((lang) => ({
        label: lang === 'ar' ? 'Translate into Arabic (العربية)' : 'Translate into English',
        checked: shown?.lang === lang,
        action: () => translate(false, false, lang),
      })),
      ...(shown ? [{ label: 'Show the original', checked: false, action: () => translate() }] : []),
      { separator: true },
      { label: 'Usually translate into English', checked: usual === 'en', action: () => store.updateSettings({ translateTo: 'en' }) },
      { label: 'Usually translate into Arabic', checked: usual === 'ar', action: () => store.updateSettings({ translateTo: 'ar' }) },
    ]);
  }
  // The translation in the article's layout, as far as it is done: each paragraph once
  // the model has translated it, and each image once the text before it is there.
  const translatedBlocks = $derived.by((): TranslationBlock[] => {
    if (!shown || !detail) return [];
    const paragraphs = shown.paragraphs;
    const layout = detail.translationLayout.length
      ? detail.translationLayout
      : paragraphs.map((_, index): TranslationBlock => ({ kind: 'text', index }));
    const out: TranslationBlock[] = [];
    let textBefore = true;
    for (const block of layout) {
      if (block.kind === 'text') {
        textBefore = paragraphs[block.index] != null;
        if (textBefore) out.push(block);
      } else if (textBefore) out.push(block);
    }
    return out;
  });
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
      const again = store.translated.get(d.id);
      if (store.feedById.get(d.feedId)?.fullText && d.url) toggleFullText(true);
      else if (again && translationTargets(d.language, store.settings.translateTo).includes(again)) translate(false, false, again);
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
      // A translation of the article before stops: the model works for the one shown.
      if (translating) api.stopTranslation();
      shown = null;
      translating = false;
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
      // The translation was of the full text: translate the feed's text instead.
      if (shown) translate(false, true, shown.lang);
      return;
    }
    if (!detail.url) return store.toast('This article has no link to load');
    fullLoading = true;
    try {
      const d = await api.fullText(id, loadImages);
      if (d.id === articleId) {
        detail = d;
        // The translation was of the feed's text: translate the full text instead.
        const lang = shown?.lang ?? store.translated.get(d.id);
        if (lang) translate(false, true, lang);
      }
    } catch (e) {
      if (!force) store.error(e);
    } finally {
      fullLoading = false;
    }
  }

  // Parts of the translation as the model finishes them.
  $effect(() => {
    let stop: (() => void) | null = null;
    let gone = false;
    on<TranslationPart>('translation-part', (part) => {
      if (!translating || !shown || part.articleId !== detail?.id || part.full !== detail.showingFullText || part.lang !== shown.lang) return;
      shown.total = part.total;
      if (part.index == null) shown.title = part.text;
      else shown.paragraphs[part.index] = part.text;
    }).then((unlisten) => (gone ? unlisten() : (stop = unlisten)));
    return () => {
      gone = true;
      stop?.();
    };
  });

  /** Shows the article translated into `lang` (the usual language for this article
   *  when none is given), or the original again. `again` asks the model anew;
   *  `keep` translates again in the same language, for other text. */
  async function translate(again = false, keep = false, lang: TranslateLang | null = null) {
    if (!detail) return;
    const d = detail;
    if (shown && !again && !keep && !lang) {
      if (translating) api.stopTranslation();
      translationRun++;
      shown = null;
      translating = false;
      store.translated.delete(d.id);
      return;
    }
    const target = lang ?? shown?.lang ?? targets[0];
    if (!target) return;
    if (!store.translateReady) {
      store.dialog = { kind: 'settings', section: 'ai' };
      return;
    }
    const run = ++translationRun;
    store.translated.set(d.id, target);
    shown = { title: null, paragraphs: [], total: 0, model: null, cut: false, lang: target };
    translating = true;
    try {
      const t = await api.translate(d.id, d.showingFullText, again, target);
      if (run === translationRun && detail?.id === d.id && shown) {
        shown = { title: t.title, paragraphs: t.paragraphs, total: t.paragraphs.length, model: t.model, cut: t.cut, lang: t.lang ?? target };
      }
    } catch (e) {
      if (run !== translationRun || detail?.id !== d.id) return;
      shown = null;
      if (errorText(e) !== 'The translation was stopped') {
        store.translated.delete(d.id);
        store.error(e);
      }
    } finally {
      if (run === translationRun && detail?.id === d.id) translating = false;
    }
  }

  function stopTranslating() {
    api.stopTranslation();
    if (shown && shown.paragraphs.length === 0) shown = null;
    translating = false;
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
      case 'translate':
        if (targets.length || shown) translate();
        else translateMenu();
        break;
      case 'browser':
        store.openExternal(detail.url);
        break;
      case 'watch':
        if (videoUrl) store.watchVideo(videoUrl);
        else store.toast('This article has no video');
        break;
      case 'listen':
        if (video?.kind === 'site') player.listen(detail.id, detail.feedId, detail.title, video.url);
        else store.toast(video ? 'This video plays inside Tidings; there is nothing to listen to separately' : 'This article has no video');
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
        {#if possible.length || shown}
          <span class="split">
            <button
              class="tool"
              class:on={!!shown}
              title={shown
                ? 'Show the original (T)'
                : !store.translateReady
                  ? 'Translate: set up a local model'
                  : targets.length
                    ? `Translate into ${LANGUAGE_NAMES[targets[0]]} with the local model (T)`
                    : 'Translate: choose a language (T)'}
              onclick={() => (shown || targets.length ? translate() : translateMenu())}
            >
              {#if translating}<span class="spinner"></span>{:else}<Icon name="languages" size={15} />{/if}
              <span>{shown ? 'Original' : targets.length ? `Translate · ${targets[0].toUpperCase()}` : 'Translate'}</span>
            </button>
            <button class="tool more" class:on={!!shown} bind:this={languageButton} title="Choose the language" aria-label="Choose the translation language" onclick={translateMenu}>
              <Icon name="chevron-down" size={12} stroke={2.2} />
            </button>
          </span>
        {/if}
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
        {#if video?.kind === 'site'}
          {@const v = video}
          <button class="icon-btn" title="Listen: play only the sound (L)" onclick={() => detail && player.listen(detail.id, detail.feedId, detail.title, v.url)}>
            <Icon name="headphones" size={16} />
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
              <a href={detail.url} onclick={(e) => { e.preventDefault(); store.openExternal(detail?.url); }}>{shown?.title ?? detail.title}</a>
            {:else}
              {shown?.title ?? detail.title}
            {/if}
          </h1>
          {#if shown?.title}<div class="original-title" dir="auto">{detail.title}</div>{/if}
          {#if detail.author}<div class="byline" dir="auto">By {detail.author}</div>{/if}
        </header>

        {#if video}
          {#key detail.id}
            <VideoCard source={video} title={detail.title} poster={thumb} articleId={detail.id} feedId={detail.feedId} />
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

        {#if shown}
          <div class="fulltext-note translation-note">
            <Icon name="languages" size={13} />
            {#if translating}
              <span class="spinner"></span> Translating into {LANGUAGE_NAMES[shown.lang]}{shown.total ? ` ${shown.paragraphs.filter(Boolean).length} of ${shown.total}` : ''} with {store.settings.translateModel || store.settings.aiModel}…
              <button class="link" onclick={stopTranslating}>Stop</button>
            {:else}
              Translated {detail.language ? `from ${LANGUAGE_NAMES[detail.language]} ` : ''}into {LANGUAGE_NAMES[shown.lang]} by {shown.model} · not checked by a person
              <button class="link" onclick={() => translate()}>Original</button>
              <button class="link" onclick={() => translate(true)}>Translate again</button>
              {#if otherLanguage}
                <button class="link" onclick={() => otherLanguage && translate(false, false, otherLanguage)}>{LANGUAGE_NAMES[otherLanguage]}</button>
              {/if}
            {/if}
          </div>
          <ArticleBody dir={shown.lang === 'ar' ? 'rtl' : 'auto'}>
            {#each translatedBlocks as block, i (i)}
              {#if block.kind === 'image'}
                <!-- One of the article's images, cleaned and proxied by sanitize::render; not from the model. -->
                {@html block.html}
              {:else}
                <!-- Plain text from the model: Svelte escapes it, so it can never be markup. -->
                <p>{shown.paragraphs[block.index]}</p>
              {/if}
            {/each}
          </ArticleBody>
          {#if shown.cut}<p class="no-content">The rest of this article is too long to translate.</p>{/if}
        {:else}
          <ArticleBody html={detail.html} />
        {/if}

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
