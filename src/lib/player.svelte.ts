// Podcast player state. The <audio> element lives in PlayerBar.svelte and registers itself here.
import { api, errorText, iconUrl, mediaUrl } from './api';
import { store } from './store.svelte';

export interface Episode {
  articleId: number;
  feedId: number;
  title: string;
  /** The audio file, or with `listen` the video page. Saved positions use it as the key. */
  url: string;
  type: string | null;
  /** The audio of a video ("Listen"): yt-dlp finds the stream for `url` at each start. */
  listen?: boolean;
}

const POSITIONS_KEY = 'tidings.positions';
const SPEEDS = [0.8, 1, 1.2, 1.5, 1.75, 2];

function loadPositions(): Record<string, number> {
  try {
    return JSON.parse(localStorage.getItem(POSITIONS_KEY) ?? '{}');
  } catch {
    return {};
  }
}

class Player {
  episode = $state<Episode | null>(null);
  playing = $state(false);
  loading = $state(false);
  time = $state(0);
  duration = $state(0);
  rate = $state<number>(Number(localStorage.getItem('tidings.rate')) || 1);
  volume = $state<number>(Number(localStorage.getItem('tidings.volume') ?? 1));
  muted = $state(false);
  error = $state<string | null>(null);

  private audio: HTMLAudioElement | null = null;
  private lastSaved = 0;
  /** Where a "Listen" stream failed and was looked up again, or null. */
  private retriedAt: number | null = null;

  attach(el: HTMLAudioElement) {
    this.audio = el;
    el.playbackRate = this.rate;
    el.volume = this.volume;
  }

  isCurrent(url: string | null | undefined): boolean {
    return !!url && this.episode?.url === url;
  }

  /** Plays an episode, or toggles it if it is already loaded. Resumes where you left off. */
  async play(ep: Episode) {
    const el = this.audio;
    if (!el) return;
    if (this.episode?.url === ep.url) return this.toggle();
    this.savePosition();
    this.episode = ep;
    this.error = null;
    this.time = 0;
    this.duration = 0;
    this.loading = true;
    this.retriedAt = null;
    this.updateMediaSession();
    const source = ep.listen ? await this.findAudio(ep.url) : ep.url;
    // Another episode may have started while yt-dlp was looking.
    if (!source || this.episode?.url !== ep.url) return;
    this.start(el, source, loadPositions()[ep.url] ?? 0);
  }

  /** Plays the audio of a video page (YouTube, Vimeo...) like a podcast episode. */
  listen(articleId: number, feedId: number, title: string, page: string) {
    return this.play({ articleId, feedId, title, url: page, type: null, listen: true });
  }

  private start(el: HTMLAudioElement, source: string, resume: number) {
    el.src = mediaUrl(source);
    el.playbackRate = this.rate;
    if (resume > 5) {
      el.addEventListener('loadedmetadata', () => (el.currentTime = resume), { once: true });
    }
    // The error event reports real failures.
    el.play().catch(() => {});
  }

  /** The audio stream of a video page, from yt-dlp, or null after an error (shown in the bar). */
  private async findAudio(page: string): Promise<string | null> {
    try {
      return (await api.audioStream(page)).url;
    } catch (err) {
      if (this.episode?.url === page) {
        this.loading = false;
        this.playing = false;
        this.error = errorText(err);
      }
      return null;
    }
  }

  pause() {
    this.audio?.pause();
  }

  toggle() {
    const el = this.audio;
    if (!el || !this.episode) return;
    if (el.paused) el.play().catch(() => {});
    else el.pause();
  }

  seek(seconds: number) {
    const el = this.audio;
    if (!el || !Number.isFinite(seconds)) return;
    el.currentTime = Math.max(0, Math.min(seconds, this.duration || seconds));
  }

  skip(delta: number) {
    if (this.audio) this.seek(this.audio.currentTime + delta);
  }

  cycleRate() {
    const next = SPEEDS[(SPEEDS.indexOf(this.rate) + 1) % SPEEDS.length] ?? 1;
    this.setRate(next);
  }

  setRate(rate: number) {
    this.rate = rate;
    if (this.audio) this.audio.playbackRate = rate;
    localStorage.setItem('tidings.rate', String(rate));
  }

  setVolume(v: number) {
    this.volume = Math.max(0, Math.min(1, v));
    this.muted = this.volume === 0;
    if (this.audio) {
      this.audio.volume = this.volume;
      this.audio.muted = this.muted;
    }
    localStorage.setItem('tidings.volume', String(this.volume));
  }

  toggleMute() {
    this.muted = !this.muted;
    if (this.audio) this.audio.muted = this.muted;
  }

  close() {
    this.savePosition();
    if (this.audio) {
      this.audio.pause();
      this.audio.removeAttribute('src');
      this.audio.load();
    }
    this.episode = null;
    this.playing = false;
    this.error = null;
  }

  // Events from the <audio> element ------------------------------------------

  onTime() {
    if (!this.audio) return;
    // WebKitGTK fires `waiting` during a seek but not always `playing` afterwards,
    // which left the spinner up while audio played. Small steps forward (not the
    // jump of a seek) mean it is playing again.
    const step = this.audio.currentTime - this.time;
    if (this.loading && !this.audio.paused && step > 0 && step < 2) this.loading = false;
    this.time = this.audio.currentTime;
    // Playing well again for a while: a later failure may look the stream up again.
    if (this.retriedAt != null && this.time > this.retriedAt + 30) this.retriedAt = null;
    if (Date.now() - this.lastSaved > 5000) this.savePosition();
  }

  onEnded() {
    const ep = this.episode;
    this.playing = false;
    if (!ep) return;
    this.forgetPosition(ep.url);
    store.markRead([ep.articleId], true);
  }

  onError() {
    const ep = this.episode;
    const el = this.audio;
    if (!ep || !el) return;
    // A video's audio address expires after some hours and only works from the IP
    // address that asked for it; with Tor that address changes every few minutes.
    // Ask yt-dlp for a new one, once, and go on from the same place.
    if (ep.listen && this.retriedAt == null && el.getAttribute('src')) {
      // If the element went back to 0 when it failed, use the last saved position.
      const at = this.time > 0 ? this.time : (loadPositions()[ep.url] ?? 0);
      this.retriedAt = at;
      this.loading = true;
      this.findAudio(ep.url).then((source) => {
        if (source && this.episode?.url === ep.url) this.start(el, source, at);
      });
      return;
    }
    this.loading = false;
    this.playing = false;
    this.error = ep.listen ? 'The audio of this video could not be played.' : 'This episode could not be played.';
  }

  savePosition() {
    const el = this.audio;
    const ep = this.episode;
    if (!el || !ep || !Number.isFinite(el.currentTime)) return;
    this.lastSaved = Date.now();
    const positions = loadPositions();
    const nearEnd = this.duration > 0 && el.currentTime > this.duration - 30;
    if (el.currentTime < 5 || nearEnd) delete positions[ep.url];
    else positions[ep.url] = Math.floor(el.currentTime);
    const keys = Object.keys(positions);
    for (const k of keys.slice(0, Math.max(0, keys.length - 300))) delete positions[k];
    localStorage.setItem(POSITIONS_KEY, JSON.stringify(positions));
  }

  private forgetPosition(url: string) {
    const positions = loadPositions();
    delete positions[url];
    localStorage.setItem(POSITIONS_KEY, JSON.stringify(positions));
  }

  /** System media keys and the macOS Now Playing widget. */
  private updateMediaSession() {
    if (!('mediaSession' in navigator) || !this.episode) return;
    const feed = store.feedById.get(this.episode.feedId);
    const art = feed ? iconUrl(feed.id, feed.iconVersion) : null;
    try {
      navigator.mediaSession.metadata = new MediaMetadata({
        title: this.episode.title,
        artist: feed?.title ?? '',
        album: 'Tidings',
        artwork: art ? [{ src: art }] : [],
      });
      navigator.mediaSession.setActionHandler('play', () => this.toggle());
      navigator.mediaSession.setActionHandler('pause', () => this.toggle());
      navigator.mediaSession.setActionHandler('seekbackward', () => this.skip(-15));
      navigator.mediaSession.setActionHandler('seekforward', () => this.skip(30));
      navigator.mediaSession.setActionHandler('seekto', (d) => d.seekTime != null && this.seek(d.seekTime));
    } catch {
      /* not supported by this web view */
    }
  }
}

export const player = new Player();

export function formatTime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return '0:00';
  const s = Math.floor(seconds % 60).toString().padStart(2, '0');
  const m = Math.floor((seconds / 60) % 60);
  const h = Math.floor(seconds / 3600);
  return h > 0 ? `${h}:${m.toString().padStart(2, '0')}:${s}` : `${m}:${s}`;
}

export function isAudio(type: string | null | undefined, url: string | null | undefined): boolean {
  if (type?.startsWith('audio/')) return true;
  return !!url && /\.(mp3|m4a|aac|ogg|oga|opus|flac|wav)(\?|$)/i.test(url);
}
