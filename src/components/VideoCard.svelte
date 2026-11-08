<script lang="ts">
  // The video part of an article: plays PeerTube videos and video files inside Tidings,
  // sends other video links (YouTube...) to mpv or IINA, and can look for a PeerTube
  // copy of those. Files stream through the Rust media proxy, like podcast audio.
  import { untrack } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import { api, mediaUrl } from '../lib/api';
  import { formatTime, player } from '../lib/player.svelte';
  import { store } from '../lib/store.svelte';
  import type { PeerTubeHit } from '../lib/types';
  import { hostOf, type VideoSource } from '../lib/util';

  let { source, title, poster = null }: { source: VideoSource; title: string; poster?: string | null } = $props();

  /** What plays inside Tidings now. `page` is what an external player gets. */
  let current = $state<{ file: string; label: string; host: string; page: string; poster: string | null } | null>(null);
  let resolving = $state<string | null>(null);
  let failed = $state(false);
  let results = $state<PeerTubeHit[] | null>(null);
  let searching = $state(false);
  let videoEl: HTMLVideoElement | undefined = $state();

  const playerName = $derived(store.videoPlayer?.name ?? 'your video player');
  const playsHere = $derived(source.kind !== 'site');

  async function playHere(page: string, kind: 'peertube' | 'file', thumb: string | null) {
    failed = false;
    if (kind === 'file') {
      current = { file: page, label: '', host: hostOf(page), page, poster: thumb };
      return;
    }
    resolving = page;
    try {
      const f = await api.peertubeVideo(page);
      current = { file: f.url, label: f.label, host: hostOf(page), page, poster: thumb };
    } catch (e) {
      store.error(e);
    } finally {
      resolving = null;
    }
  }

  function main() {
    if (source.kind === 'site') store.watchVideo(source.url);
    else playHere(source.url, source.kind, poster);
  }

  async function findOnPeerTube() {
    searching = true;
    results = null;
    try {
      results = await api.searchPeertube(title);
    } catch (e) {
      store.error(e);
    } finally {
      searching = false;
    }
  }

  // One thing plays at a time: a podcast that starts pauses the video, and the reverse.
  $effect(() => {
    if (player.playing) untrack(() => videoEl)?.pause();
  });
</script>

<div class="video-block">
  {#if current}
    <div class="video-frame">
      <!-- Feeds carry no captions for these files. -->
      <!-- svelte-ignore a11y_media_has_caption -->
      <video
        bind:this={videoEl}
        src={mediaUrl(current.file)}
        poster={current.poster ?? undefined}
        controls
        autoplay
        playsinline
        preload="metadata"
        onplay={() => player.pause()}
        onerror={() => (failed = true)}
      ></video>
    </div>
    <div class="video-meta">
      <span class="video-note">{current.label ? `${current.label} · ` : ''}{current.host} · streamed privately through Tidings</span>
      <button class="icon-btn" title={`Watch in ${playerName}`} onclick={() => store.watchVideo(current?.page)}>
        <Icon name="external" size={14} />
      </button>
      <button class="icon-btn" title="Close the video" onclick={() => (current = null)}>
        <Icon name="x" size={14} />
      </button>
    </div>
    {#if failed}
      <div class="notice video-failed">
        <Icon name="alert" size={16} />
        <span>This video cannot play inside Tidings.</span>
        <button class="btn small" onclick={() => store.watchVideo(current?.page)}>Watch in {playerName}</button>
      </div>
    {/if}
  {:else}
    <div class="enclosure episode-card">
      <button class="episode-play" aria-label={playsHere ? 'Play video' : 'Watch video'} onclick={main} disabled={resolving === source.url}>
        {#if resolving === source.url}<span class="spinner light"></span>{:else}<Icon name="play" size={18} stroke={0} />{/if}
      </button>
      <span class="enclosure-text">
        <strong>{playsHere ? 'Play video' : 'Watch video'}</strong>
        <span>
          {hostOf(source.url)} ·
          {#if playsHere}plays here, streamed privately through Tidings
          {:else if store.videoPlayer}plays in {store.videoPlayer.name}, without the site's own player
          {:else}needs mpv or IINA{/if}
        </span>
      </span>
      {#if source.kind === 'site'}
        <button class="btn small ghost" title="Search PeerTube for this title (sends the title to Sepia Search)" onclick={findOnPeerTube} disabled={searching}>
          {#if searching}<span class="spinner"></span>{:else}<Icon name="search" size={13} />{/if}Find on PeerTube
        </button>
      {:else}
        <button class="icon-btn" title={`Watch in ${playerName}`} onclick={() => store.watchVideo(source.url)}>
          <Icon name="external" size={14} />
        </button>
      {/if}
      <button class="icon-btn" title="Open in browser" onclick={() => store.openExternal(source.url)}>
        <Icon name="globe" size={14} />
      </button>
    </div>
  {/if}

  {#if results}
    <div class="pt-results">
      <div class="pt-head">
        <strong>{results.length ? 'On PeerTube' : 'No PeerTube video found for this title'}</strong>
        <span class="pt-source">Sepia Search</span>
        <button class="icon-btn" title="Close the results" onclick={() => (results = null)}><Icon name="x" size={13} /></button>
      </div>
      {#if results.length}
        <p class="hint pt-hint">Matched by title only. Check the channel and length before you watch.</p>
      {/if}
      {#each results as hit (hit.url)}
        <div class="pt-hit" class:on={current?.page === hit.url}>
          <button class="pt-main" title="Play here" onclick={() => playHere(hit.url, 'peertube', hit.thumbnail)}>
            {#if hit.thumbnail}
              <img class="pt-thumb" src={hit.thumbnail} alt="" />
            {:else}
              <span class="pt-thumb"><Icon name="video" size={16} /></span>
            {/if}
            <span class="pt-text">
              <span class="pt-title" dir="auto">{hit.title}</span>
              <span class="pt-sub">{hit.channel ? `${hit.channel} · ` : ''}{hit.host} · {formatTime(hit.duration)}</span>
            </span>
            {#if resolving === hit.url}<span class="spinner"></span>{:else}<Icon name="play" size={13} stroke={0} />{/if}
          </button>
          <button class="icon-btn" title={`Watch in ${playerName}`} onclick={() => store.watchVideo(hit.url)}>
            <Icon name="external" size={14} />
          </button>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .video-block {
    margin: 0 0 24px;
  }
  .video-block :global(.enclosure) {
    margin-bottom: 0;
  }
  .video-frame {
    border-radius: 12px;
    overflow: hidden;
    background: #000;
    aspect-ratio: 16 / 9;
  }
  video {
    display: block;
    width: 100%;
    height: 100%;
    background: #000;
  }
  .video-meta {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-top: 6px;
    font-size: 12px;
    color: var(--text-3);
  }
  .video-note {
    flex: 1;
  }
  .video-failed {
    margin: 10px 0 0;
  }
  .spinner.light {
    border-color: rgb(255 255 255 / 0.35);
    border-top-color: #fff;
  }
  .pt-results {
    margin-top: 10px;
    padding: 8px;
    border: 1px solid var(--border);
    border-radius: 12px;
    background: var(--bg-muted);
  }
  .pt-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 2px 4px 2px 6px;
    font-size: 13px;
  }
  .pt-source {
    flex: 1;
    font-size: 11px;
    color: var(--text-3);
  }
  .pt-hint {
    margin: 0 6px 6px;
  }
  .pt-hit {
    display: flex;
    align-items: center;
    gap: 4px;
    border-radius: 8px;
  }
  .pt-hit.on {
    background: var(--accent-softer);
  }
  .pt-main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px;
    border: none;
    border-radius: 8px;
    background: none;
    color: var(--text);
    text-align: left;
  }
  .pt-main:hover {
    background: var(--bg-hover);
  }
  .pt-thumb {
    flex: none;
    width: 72px;
    height: 40px;
    border-radius: 6px;
    object-fit: cover;
    display: grid;
    place-items: center;
    background: var(--bg-hover);
    color: var(--text-3);
  }
  .pt-text {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 2px;
  }
  .pt-title {
    font-size: 13px;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pt-sub {
    font-size: 11.5px;
    color: var(--text-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
