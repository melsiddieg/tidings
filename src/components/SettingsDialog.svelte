<script lang="ts">
  import { untrack } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import Modal from './Modal.svelte';
  import { store } from '../lib/store.svelte';
  import type { Settings } from '../lib/types';
  import { copyText } from '../lib/util';

  let { section = 'general' }: { section?: string } = $props();
  let current = $state(untrack(() => section));

  const SECTIONS = [
    { id: 'general', label: 'General', icon: 'settings' },
    { id: 'reading', label: 'Reading', icon: 'book' },
    { id: 'appearance', label: 'Appearance', icon: 'palette' },
    { id: 'privacy', label: 'Privacy & network', icon: 'shield' },
    { id: 'storage', label: 'Archive & data', icon: 'archive' },
    { id: 'about', label: 'About', icon: 'info' },
  ];

  const ACCENTS: Settings['accent'][] = ['ember', 'ocean', 'forest', 'violet', 'rose', 'graphite'];
  const THEMES: { id: Settings['theme']; label: string }[] = [
    { id: 'system', label: 'System' },
    { id: 'light', label: 'Light' },
    { id: 'dark', label: 'Dark' },
    { id: 'sepia', label: 'Sepia' },
  ];

  const s = $derived(store.settings);

  // Text fields are saved when they lose focus or on Enter.
  let proxy = $state(store.settings.proxy);
  let userAgent = $state(store.settings.userAgent);
  let retentionCount = $state(store.settings.retentionCount);
  let retentionDays = $state(store.settings.retentionDays);

  function set<K extends keyof Settings>(key: K, value: Settings[K]) {
    store.updateSettings({ [key]: value } as Partial<Settings>);
  }

  async function saveProxy() {
    if (proxy.trim() === s.proxy) return;
    if (await store.updateSettings({ proxy: proxy.trim() })) store.toast(proxy.trim() ? 'Proxy enabled' : 'Proxy removed', 'success');
    else proxy = s.proxy;
  }

  async function saveUserAgent() {
    if (userAgent.trim() === s.userAgent) return;
    if (await store.updateSettings({ userAgent: userAgent.trim() })) userAgent = store.settings.userAgent;
    else userAgent = s.userAgent;
  }

  function onEnter(fn: () => void) {
    return (e: KeyboardEvent) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        fn();
      }
    };
  }
</script>

{#snippet toggle(key: 'refreshOnStartup' | 'dockBadge' | 'loadImages' | 'stripTracking' | 'httpsOnly', title: string, hint: string)}
  <div class="row">
    <div class="row-text">
      <span class="row-title">{title}</span>
      {#if hint}<span class="hint">{hint}</span>{/if}
    </div>
    <button class="switch" class:on={s[key]} aria-pressed={s[key]} aria-label={title} onclick={() => set(key, !s[key])}></button>
  </div>
{/snippet}

<Modal width={760} padded={false}>
  <div class="settings">
    <nav>
      <h2>Settings</h2>
      {#each SECTIONS as sec (sec.id)}
        <button class="nav-item" class:on={current === sec.id} onclick={() => (current = sec.id)}>
          <Icon name={sec.icon} size={15} />{sec.label}
        </button>
      {/each}
    </nav>

    <div class="content">
      <div class="content-head">
        <h3>{SECTIONS.find((x) => x.id === current)?.label}</h3>
        <button class="icon-btn" aria-label="Close" onclick={() => (store.dialog = null)}><Icon name="x" size={16} /></button>
      </div>

      {#if current === 'general'}
        <div class="card">
          <div class="row">
            <div class="row-text">
              <span class="row-title">Check for new articles</span>
              <span class="hint">Feeds can override this in their properties.</span>
            </div>
            <select class="input compact" value={String(s.refreshInterval)} onchange={(e) => set('refreshInterval', Number(e.currentTarget.value))}>
              <option value="0">Manually only</option>
              <option value="15">Every 15 minutes</option>
              <option value="30">Every 30 minutes</option>
              <option value="60">Every hour</option>
              <option value="120">Every 2 hours</option>
              <option value="240">Every 4 hours</option>
              <option value="720">Every 12 hours</option>
              <option value="1440">Once a day</option>
            </select>
          </div>
          {@render toggle('refreshOnStartup', 'Refresh all feeds at startup', '')}
          <div class="row">
            <div class="row-text">
              <span class="row-title">Parallel downloads</span>
              <span class="hint">How many feeds are fetched at the same time.</span>
            </div>
            <select class="input compact" value={String(s.concurrency)} onchange={(e) => set('concurrency', Number(e.currentTarget.value))}>
              {#each [2, 4, 6, 8, 12] as n (n)}<option value={String(n)}>{n}</option>{/each}
            </select>
          </div>
          {@render toggle('dockBadge', 'Show unread count on the app icon', '')}
        </div>
      {:else if current === 'reading'}
        <div class="card">
          <div class="row">
            <div class="row-text">
              <span class="row-title">Mark articles as read</span>
              <span class="hint">When you select an article in the list.</span>
            </div>
            <select class="input compact" value={String(s.markReadDelay)} onchange={(e) => set('markReadDelay', Number(e.currentTarget.value))}>
              <option value="0">Immediately</option>
              <option value="2">After 2 seconds</option>
              <option value="5">After 5 seconds</option>
              <option value="10">After 10 seconds</option>
            </select>
          </div>
          <div class="row">
            <div class="row-text">
              <span class="row-title">Links inside articles</span>
              <span class="hint">⌘/Ctrl-click opens a background tab. Shift-click opens your browser.</span>
            </div>
            <div class="segmented">
              <button class:on={s.linkTarget === 'tab'} onclick={() => set('linkTarget', 'tab')}>Reader tab</button>
              <button class:on={s.linkTarget === 'browser'} onclick={() => set('linkTarget', 'browser')}>Browser</button>
            </div>
          </div>
          <div class="row">
            <div class="row-text">
              <span class="row-title">Layout</span>
              <span class="hint">Widescreen: three columns. Normal: list above the article. Combined: full articles in one stream.</span>
            </div>
            <div class="segmented">
              {#each ['widescreen', 'normal', 'combined'] as const as m (m)}
                <button class:on={s.viewMode === m} title={m} onclick={() => set('viewMode', m)}><Icon name={m} size={15} /></button>
              {/each}
            </div>
          </div>
        </div>
      {:else if current === 'appearance'}
        <div class="card">
          <div class="row stack">
            <span class="row-title">Theme</span>
            <div class="themes">
              {#each THEMES as t (t.id)}
                <button class="theme" class:on={s.theme === t.id} onclick={() => set('theme', t.id)}>
                  <span class="swatch {t.id}"><span></span><span></span><span></span></span>
                  {t.label}
                </button>
              {/each}
            </div>
          </div>
          <div class="row">
            <span class="row-title">Accent color</span>
            <div class="accents">
              {#each ACCENTS as a (a)}
                <button class="accent-dot {a}" class:on={s.accent === a} aria-label={a} title={a} onclick={() => set('accent', a)}></button>
              {/each}
            </div>
          </div>
          <div class="row">
            <span class="row-title">Article font</span>
            <div class="segmented">
              <button class:on={s.readerFont === 'serif'} onclick={() => set('readerFont', 'serif')} style="font-family: var(--font-serif)">Serif</button>
              <button class:on={s.readerFont === 'sans'} onclick={() => set('readerFont', 'sans')}>Sans</button>
              <button class:on={s.readerFont === 'mono'} onclick={() => set('readerFont', 'mono')} style="font-family: var(--font-mono)">Mono</button>
            </div>
          </div>
          <div class="row">
            <span class="row-title">Text size</span>
            <div class="stepper">
              <button class="icon-btn" onclick={() => set('readerSize', Math.max(13, s.readerSize - 1))} aria-label="Smaller"><Icon name="minus" size={14} /></button>
              <span class="size">{s.readerSize}px</span>
              <button class="icon-btn" onclick={() => set('readerSize', Math.min(28, s.readerSize + 1))} aria-label="Larger"><Icon name="plus" size={14} /></button>
            </div>
          </div>
          <div class="preview prose">
            <p><strong>The quick brown fox</strong> jumps over the lazy dog. Good typography makes long reading comfortable.</p>
          </div>
        </div>
      {:else if current === 'privacy'}
        <div class="pledge">
          <Icon name="shield" size={18} />
          <div>
            <strong>No telemetry. No accounts. No tracking.</strong>
            <span>Tidings only connects to the feeds you subscribe to, and to the pages and images of articles you open. Article HTML is cleaned in Rust and shown without scripts, cookies or a referrer.</span>
          </div>
        </div>
        <div class="card">
          {@render toggle('loadImages', 'Load remote images', 'Images are downloaded by Tidings itself through its private image proxy, never by the web view. Off: images show only when you ask.')}
          {@render toggle('stripTracking', 'Remove tracking parameters from links', 'Strips utm_*, fbclid, gclid and similar from article links.')}
          {@render toggle('httpsOnly', 'Only use secure connections (HTTPS)', 'Feeds and pages served over plain HTTP will fail to load.')}
        </div>
        <div class="card">
          <div class="row stack">
            <div class="row-text">
              <span class="row-title">Proxy</span>
              <span class="hint">Route all traffic through a proxy. For Tor use <code class="selectable">socks5h://127.0.0.1:9050</code>. Leave empty for a direct connection.</span>
            </div>
            <input class="input mono" bind:value={proxy} onblur={saveProxy} onkeydown={onEnter(saveProxy)} placeholder="socks5h://127.0.0.1:9050" spellcheck="false" autocomplete="off" />
          </div>
          <div class="row stack">
            <div class="row-text">
              <span class="row-title">User agent</span>
              <span class="hint">Sent to servers with every request. The default does not reveal your system.</span>
            </div>
            <div class="inline">
              <input class="input mono" bind:value={userAgent} onblur={saveUserAgent} onkeydown={onEnter(saveUserAgent)} spellcheck="false" autocomplete="off" />
              <button class="btn small" onclick={() => { userAgent = ''; saveUserAgent(); }}>Reset</button>
            </div>
          </div>
        </div>
      {:else if current === 'storage'}
        <div class="card">
          <div class="row stack">
            <div class="row-text">
              <span class="row-title">Default archive</span>
              <span class="hint">Old articles are removed automatically. Starred articles are always kept. Feeds can override this.</span>
            </div>
            <div class="archive">
              <label class="radio"><input type="radio" checked={s.retentionMode === 'all'} onchange={() => set('retentionMode', 'all')} /> Keep everything</label>
              <label class="radio">
                <input type="radio" checked={s.retentionMode === 'count'} onchange={() => set('retentionMode', 'count')} />
                Keep the newest
                <input class="input tiny" type="number" min="10" bind:value={retentionCount} onchange={() => set('retentionCount', Number(retentionCount))} />
                articles per feed
              </label>
              <label class="radio">
                <input type="radio" checked={s.retentionMode === 'days'} onchange={() => set('retentionMode', 'days')} />
                Keep articles for
                <input class="input tiny" type="number" min="1" bind:value={retentionDays} onchange={() => set('retentionDays', Number(retentionDays))} />
                days
              </label>
            </div>
          </div>
        </div>
        <div class="card">
          <div class="row">
            <div class="row-text">
              <span class="row-title">Subscriptions</span>
              <span class="hint">OPML works with Akregator, NetNewsWire, Feedly, Inoreader and most readers.</span>
            </div>
            <div class="inline">
              <button class="btn small" onclick={() => store.importOpml()}><Icon name="download" size={13} />Import</button>
              <button class="btn small" onclick={() => store.exportOpml()}><Icon name="upload" size={13} />Export</button>
            </div>
          </div>
          <div class="row stack">
            <div class="row-text">
              <span class="row-title">Data folder</span>
              <span class="hint">Everything Tidings stores lives in one SQLite database here, readable only by your user account.</span>
            </div>
            <div class="inline">
              <code class="path selectable">{store.info?.dataDir ?? '—'}</code>
              <button class="btn small" onclick={() => store.info && copyText(store.info.dataDir).then(() => store.toast('Path copied'))}><Icon name="copy" size={13} />Copy</button>
            </div>
          </div>
        </div>
      {:else if current === 'about'}
        <div class="about">
          <div class="about-mark"><Icon name="rss" size={30} stroke={2.4} /></div>
          <h4>Tidings</h4>
          <p class="version">Version {store.info?.version ?? '—'}</p>
          <p>A private, secure feed reader for macOS and Linux, inspired by KDE’s Akregator.</p>
          <ul>
            <li><Icon name="check" size={14} />RSS, Atom and JSON Feed with folders and OPML</li>
            <li><Icon name="check" size={14} />Articles and web pages in tabs, in a script-free reader mode</li>
            <li><Icon name="check" size={14} />No telemetry, no analytics, no update checks, no accounts</li>
            <li><Icon name="check" size={14} />Your data stays in a local database on this computer</li>
          </ul>
        </div>
      {/if}
    </div>
  </div>
</Modal>

<style>
  .settings {
    display: grid;
    grid-template-columns: 200px 1fr;
    height: min(600px, calc(100vh - 48px));
  }
  nav {
    padding: 18px 10px;
    background: var(--bg-sidebar);
    border-right: 1px solid var(--border);
    display: grid;
    align-content: start;
    gap: 2px;
  }
  nav h2 {
    margin: 0 10px 12px;
    font-size: 15px;
  }
  .nav-item {
    display: flex;
    align-items: center;
    gap: 9px;
    height: 30px;
    padding: 0 10px;
    border: none;
    border-radius: 7px;
    background: transparent;
    color: var(--text-2);
    font-size: 13px;
    text-align: left;
  }
  .nav-item:hover {
    background: var(--bg-hover);
    color: var(--text);
  }
  .nav-item.on {
    background: var(--accent-soft);
    color: var(--text);
  }
  .nav-item.on :global(svg) {
    color: var(--accent);
  }
  .content {
    overflow-y: auto;
    padding: 14px 24px 28px;
    display: grid;
    align-content: start;
    gap: 14px;
  }
  .content-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-right: -10px;
  }
  h3 {
    margin: 0;
    font-size: 17px;
    letter-spacing: -0.015em;
  }
  .card {
    border-radius: 12px;
    border: 1px solid var(--border);
    background: var(--bg-content);
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 18px;
    padding: 12px 14px;
  }
  .row + .row,
  .row + .preview {
    border-top: 1px solid var(--border);
  }
  .row.stack {
    flex-direction: column;
    align-items: stretch;
    gap: 10px;
  }
  .row-text {
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  .row-title {
    font-size: 13px;
    font-weight: 500;
  }
  .compact {
    width: auto;
    height: 30px;
    flex: none;
  }
  .mono {
    font-family: var(--font-mono);
    font-size: 12px;
  }
  .inline {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  code {
    font-family: var(--font-mono);
    font-size: 11.5px;
  }
  .path {
    flex: 1;
    min-width: 0;
    padding: 6px 9px;
    border-radius: 7px;
    background: var(--bg-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .themes {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 10px;
  }
  .theme {
    display: grid;
    gap: 7px;
    justify-items: center;
    padding: 0;
    border: none;
    background: none;
    color: var(--text-2);
    font-size: 12px;
  }
  .theme.on {
    color: var(--text);
    font-weight: 600;
  }
  .swatch {
    display: grid;
    grid-template-columns: 1fr 1.4fr 2fr;
    width: 100%;
    height: 56px;
    border-radius: 10px;
    overflow: hidden;
    box-shadow: 0 0 0 1px var(--border-strong);
    transition: box-shadow 0.15s;
  }
  .theme.on .swatch {
    box-shadow: 0 0 0 2px var(--accent);
  }
  .swatch.light span:nth-child(1) { background: #f3f0eb; }
  .swatch.light span:nth-child(2) { background: #faf8f5; }
  .swatch.light span:nth-child(3) { background: #ffffff; }
  .swatch.dark span:nth-child(1) { background: #151517; }
  .swatch.dark span:nth-child(2) { background: #19191c; }
  .swatch.dark span:nth-child(3) { background: #1d1d21; }
  .swatch.sepia span:nth-child(1) { background: #eee5d3; }
  .swatch.sepia span:nth-child(2) { background: #f5eee0; }
  .swatch.sepia span:nth-child(3) { background: #fbf6ec; }
  .swatch.system {
    background: linear-gradient(135deg, #f6f3ee 50%, #1b1b1e 50%);
  }
  .accents {
    display: flex;
    gap: 10px;
  }
  .accent-dot {
    width: 22px;
    height: 22px;
    border-radius: 50%;
    border: none;
    padding: 0;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.12);
    transition: transform 0.15s var(--ease), box-shadow 0.15s;
  }
  .accent-dot:hover {
    transform: scale(1.1);
  }
  .accent-dot.on {
    box-shadow: 0 0 0 2px var(--bg-content), 0 0 0 4px currentColor;
  }
  .accent-dot.ember { background: #e0552b; color: #e0552b; }
  .accent-dot.ocean { background: #2563eb; color: #2563eb; }
  .accent-dot.forest { background: #16803c; color: #16803c; }
  .accent-dot.violet { background: #7c3aed; color: #7c3aed; }
  .accent-dot.rose { background: #e11d48; color: #e11d48; }
  .accent-dot.graphite { background: #4b5563; color: #4b5563; }
  .stepper {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .size {
    min-width: 44px;
    text-align: center;
    font-variant-numeric: tabular-nums;
    font-weight: 600;
  }
  .preview {
    padding: 14px 16px;
  }
  .preview p {
    margin: 0;
  }
  .pledge {
    display: flex;
    gap: 12px;
    padding: 14px 16px;
    border-radius: 12px;
    background: color-mix(in srgb, var(--success) 9%, transparent);
    border: 1px solid color-mix(in srgb, var(--success) 22%, transparent);
  }
  .pledge > :global(svg) {
    color: var(--success);
    margin-top: 1px;
  }
  .pledge div {
    display: grid;
    gap: 4px;
    font-size: 12.5px;
    color: var(--text-2);
    line-height: 1.5;
  }
  .pledge strong {
    color: var(--text);
    font-size: 13px;
  }
  .archive {
    display: grid;
    gap: 8px;
  }
  .radio {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    color: var(--text-2);
  }
  .radio input[type='radio'] {
    accent-color: var(--accent);
    margin: 0;
  }
  .tiny {
    width: 80px;
    height: 28px;
  }
  .about {
    display: grid;
    justify-items: center;
    text-align: center;
    padding: 28px 12px;
  }
  .about-mark {
    display: grid;
    place-items: center;
    width: 64px;
    height: 64px;
    border-radius: 18px;
    color: #fff;
    background: linear-gradient(145deg, #ffa061, #f4683c 55%, #d2402e);
    box-shadow: 0 10px 26px -8px rgba(210, 64, 46, 0.55);
  }
  .about h4 {
    margin: 14px 0 2px;
    font-size: 20px;
  }
  .version {
    margin: 0 0 12px;
    color: var(--text-3);
    font-size: 12px;
  }
  .about p {
    max-width: 420px;
    color: var(--text-2);
  }
  .about ul {
    list-style: none;
    padding: 0;
    margin: 8px 0 0;
    display: grid;
    gap: 8px;
    text-align: left;
  }
  .about li {
    display: flex;
    gap: 8px;
    align-items: center;
    color: var(--text-2);
  }
  .about li :global(svg) {
    color: var(--success);
  }
</style>
