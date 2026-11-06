<script lang="ts">
  import { fly } from 'svelte/transition';
  import Icon from '../lib/Icon.svelte';
  import FeedIcon from './FeedIcon.svelte';
  import { player, formatTime } from '../lib/player.svelte';
  import { store } from '../lib/store.svelte';

  let audio: HTMLAudioElement | undefined = $state();
  let scrubbing = $state<number | null>(null);

  $effect(() => {
    if (audio) player.attach(audio);
  });

  const ep = $derived(player.episode);
  const feed = $derived(ep ? store.feedById.get(ep.feedId) : undefined);
  const shown = $derived(scrubbing ?? player.time);
  const progress = $derived(player.duration > 0 ? (shown / player.duration) * 100 : 0);

  function onSeekInput(e: Event) {
    scrubbing = Number((e.currentTarget as HTMLInputElement).value);
  }
  function onSeekChange(e: Event) {
    player.seek(Number((e.currentTarget as HTMLInputElement).value));
    scrubbing = null;
  }

  function openArticle() {
    if (ep) store.openArticleTab(ep.articleId);
  }
</script>

<audio
  bind:this={audio}
  preload="metadata"
  ontimeupdate={() => player.onTime()}
  ondurationchange={() => audio && Number.isFinite(audio.duration) && (player.duration = audio.duration)}
  onplay={() => (player.playing = true)}
  onpause={() => {
    player.playing = false;
    player.savePosition();
  }}
  onwaiting={() => (player.loading = true)}
  onplaying={() => (player.loading = false)}
  oncanplay={() => (player.loading = false)}
  onended={() => player.onEnded()}
  onerror={() => player.onError()}
></audio>

{#if ep}
  <div class="player" transition:fly={{ y: 20, duration: 200 }}>
    <button class="episode" onclick={openArticle} title="Show episode notes">
      <span class="art"><FeedIcon {feed} size={38} /></span>
      <span class="text">
        <strong dir="auto">{ep.title}</strong>
        <span>{#if player.error}<em>{player.error}</em>{:else}{feed?.title ?? ''}{/if}</span>
      </span>
    </button>

    <div class="transport">
      <button class="icon-btn" title="Back 15 seconds" onclick={() => player.skip(-15)}>
        <Icon name="back" size={18} /><span class="skip-label">15</span>
      </button>
      <button class="play" title={player.playing ? 'Pause (Shift+P)' : 'Play (Shift+P)'} onclick={() => player.toggle()}>
        {#if player.loading && player.playing}
          <span class="spinner light"></span>
        {:else}
          <Icon name={player.playing ? 'pause-fill' : 'play'} size={18} stroke={0} />
        {/if}
      </button>
      <button class="icon-btn" title="Forward 30 seconds" onclick={() => player.skip(30)}>
        <Icon name="forward" size={18} /><span class="skip-label">30</span>
      </button>
    </div>

    <div class="timeline">
      <span class="time">{formatTime(shown)}</span>
      <input
        class="seek"
        type="range"
        min="0"
        max={player.duration || 0}
        step="1"
        value={shown}
        style:--progress="{progress}%"
        oninput={onSeekInput}
        onchange={onSeekChange}
        disabled={!player.duration}
        aria-label="Position"
      />
      <span class="time">-{formatTime(Math.max(0, player.duration - shown))}</span>
    </div>

    <div class="extras">
      <button class="speed" title="Playback speed" onclick={() => player.cycleRate()}>{player.rate}×</button>
      <button class="icon-btn" title={player.muted ? 'Unmute' : 'Mute'} onclick={() => player.toggleMute()}>
        <Icon name={player.muted || player.volume === 0 ? 'volume-off' : 'volume'} size={16} />
      </button>
      <input
        class="volume"
        type="range"
        min="0"
        max="1"
        step="0.05"
        value={player.muted ? 0 : player.volume}
        style:--progress="{(player.muted ? 0 : player.volume) * 100}%"
        oninput={(e) => player.setVolume(Number(e.currentTarget.value))}
        aria-label="Volume"
      />
      {#if player.error}
        <button class="btn small" onclick={() => store.openExternal(ep.url)}><Icon name="globe" size={13} />Open</button>
      {/if}
      <button class="icon-btn" title="Close player" onclick={() => player.close()}><Icon name="x" size={15} /></button>
    </div>
  </div>
{/if}

<style>
  .player {
    display: grid;
    grid-template-columns: minmax(180px, 1fr) auto minmax(220px, 2fr) auto;
    align-items: center;
    gap: 18px;
    height: 64px;
    padding: 0 14px 0 12px;
    background: var(--bg-elevated);
    border-top: 1px solid var(--border);
    box-shadow: 0 -6px 20px -12px rgba(0, 0, 0, 0.25);
  }
  .episode {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    padding: 4px;
    border: none;
    border-radius: 9px;
    background: transparent;
    text-align: left;
  }
  .episode:hover {
    background: var(--bg-hover);
  }
  .art :global(.feed-icon) {
    border-radius: 8px;
    box-shadow: var(--shadow-sm);
  }
  .text {
    display: grid;
    min-width: 0;
  }
  .text strong {
    font-size: 13px;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .text span {
    font-size: 11.5px;
    color: var(--text-3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .text em {
    color: var(--danger);
    font-style: normal;
  }
  .transport {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .transport .icon-btn {
    position: relative;
    width: 34px;
    height: 34px;
  }
  .skip-label {
    position: absolute;
    font-size: 7.5px;
    font-weight: 700;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -42%);
  }
  .play {
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border: none;
    border-radius: 50%;
    background: var(--accent);
    color: #fff;
    box-shadow: 0 2px 8px rgb(var(--accent-rgb) / 0.4);
    transition: transform 0.1s var(--ease), filter 0.15s;
  }
  :global([data-theme='dark']) .play {
    color: #1a1410;
  }
  .play:hover {
    filter: brightness(1.07);
  }
  .play:active {
    transform: scale(0.95);
  }
  .spinner.light {
    border-color: rgba(255, 255, 255, 0.35);
    border-top-color: #fff;
  }
  .timeline {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  .time {
    font-size: 11.5px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
    min-width: 46px;
    text-align: center;
  }
  input[type='range'] {
    -webkit-appearance: none;
    appearance: none;
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(to right, var(--accent) var(--progress), var(--border-strong) var(--progress));
    outline: none;
  }
  input[type='range']::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--bg-elevated);
    box-shadow: 0 0 0 1px var(--border-strong), 0 1px 3px rgba(0, 0, 0, 0.3);
    opacity: 0;
    transition: opacity 0.15s;
  }
  input[type='range']:hover::-webkit-slider-thumb,
  input[type='range']:active::-webkit-slider-thumb {
    opacity: 1;
  }
  .seek {
    flex: 1;
    min-width: 0;
  }
  .extras {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .speed {
    min-width: 42px;
    height: 26px;
    padding: 0 8px;
    border-radius: 7px;
    border: 1px solid var(--border-strong);
    background: transparent;
    color: var(--text-2);
    font-size: 12px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .speed:hover {
    color: var(--text);
    background: var(--bg-hover);
  }
  .volume {
    width: 72px;
  }
</style>
