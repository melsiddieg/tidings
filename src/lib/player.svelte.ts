// Podcast player state. The <audio> element lives in PlayerBar.svelte and registers itself here.
import { iconUrl, mediaUrl } from './api';
import { store } from './store.svelte';

export interface Episode {
  articleId: number;
  feedId: number;
  title: string;
  url: string;
  type: string | null;
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
    el.src = mediaUrl(ep.url);
    el.playbackRate = this.rate;
    const resume = loadPositions()[ep.url] ?? 0;
    if (resume > 5) {
      el.addEventListener('loadedmetadata', () => (el.currentTime = resume), { once: true });
    }
    this.updateMediaSession();
    try {
      await el.play();
    } catch {
      /* the error event reports real failures */
    }
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
    if (!this.episode) return;
    this.loading = false;
    this.playing = false;
    this.error = 'This episode could not be played.';
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
