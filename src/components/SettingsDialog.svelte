<script lang="ts">
  import { untrack } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import Modal from './Modal.svelte';
  import { store } from '../lib/store.svelte';
  import type { ComponentCheck, Settings } from '../lib/types';
  import { api, errorText } from '../lib/api';
  import { copyText } from '../lib/util';

  let { section = 'general' }: { section?: string } = $props();
  let current = $state(untrack(() => section));

  const SECTIONS = [
    { id: 'general', label: 'General', icon: 'settings' },
    { id: 'reading', label: 'Reading', icon: 'book' },
    { id: 'appearance', label: 'Appearance', icon: 'palette' },
    { id: 'privacy', label: 'Privacy & network', icon: 'shield' },
    { id: 'security', label: 'Security', icon: 'lock' },
    { id: 'ai', label: 'AI briefs', icon: 'sparkles' },
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
  let videoPlayer = $state(store.settings.videoPlayer);
  let aiEndpoint = $state(store.settings.aiEndpoint);
  let aiModel = $state(store.settings.aiModel);
  let aiModels = $state<string[] | null>(null);
  let aiModelsError = $state<string | null>(null);
  let aiLoading = $state(false);
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

  // The user may have installed a player since startup.
  store.findVideoPlayer();

  const STATUS: Record<ComponentCheck['status'], string> = {
    ok: 'Up to date',
    old: 'Old',
    update: 'Update available',
    missing: 'Not installed',
    unknown: 'Unknown',
    info: '',
  };
  const isMac = $derived(store.info?.platform === 'macos');

  // Fresh offline results each time the section opens, unless "Check now" has run.
  $effect(() => {
    if (current === 'security' && !untrack(() => store.componentsCheckedOnline)) store.checkComponents();
  });

  async function saveVideoPlayer() {
    if (videoPlayer.trim() === s.videoPlayer) return;
    if (!(await store.updateSettings({ videoPlayer: videoPlayer.trim() }))) videoPlayer = s.videoPlayer;
  }

  const AI_PROVIDERS: { id: Settings['aiProvider']; label: string; address: string }[] = [
    { id: '', label: 'Off', address: '' },
    { id: 'ollama', label: 'Ollama', address: 'http://127.0.0.1:11434' },
    { id: 'openai', label: 'LM Studio, llama.cpp or another OpenAI-style server', address: 'http://127.0.0.1:1234' },
  ];
  const aiDefault = $derived(AI_PROVIDERS.find((p) => p.id === s.aiProvider)?.address ?? '');

  /** Asks the model server which models it has. Also shows whether it answers. */
  async function loadAiModels() {
    if (!s.aiProvider) return;
    aiLoading = true;
    aiModelsError = null;
    try {
      aiModels = await api.aiModels();
      if (aiModels.length === 0) aiModelsError = 'The server answered, but it has no models. Download one first (for example: ollama pull qwen3:4b).';
    } catch (e) {
      aiModels = null;
      aiModelsError = errorText(e);
    } finally {
      aiLoading = false;
    }
  }

  async function setAiProvider(id: Settings['aiProvider']) {
    if (id === s.aiProvider) return;
    aiModels = null;
    aiModelsError = null;
    if (await store.updateSettings({ aiProvider: id, aiEndpoint: '', aiModel: '' })) {
      aiEndpoint = '';
      aiModel = '';
      loadAiModels();
    }
  }

  async function saveAiEndpoint() {
    if (aiEndpoint.trim() === s.aiEndpoint) return;
    if (await store.updateSettings({ aiEndpoint: aiEndpoint.trim() })) loadAiModels();
    else aiEndpoint = s.aiEndpoint;
  }

  async function saveAiModel(name: string) {
    aiModel = name.trim();
    if (aiModel === s.aiModel) return;
    if (!(await store.updateSettings({ aiModel }))) aiModel = s.aiModel;
  }

  // Ask the server for its models when the section opens.
  $effect(() => {
    if (current === 'ai' && untrack(() => s.aiProvider && aiModels == null && !aiLoading)) loadAiModels();
  });

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
          <div class="row stack">
            <div class="row-text">
              <span class="row-title">Video player</span>
              <span class="hint">YouTube, Vimeo, PeerTube and similar videos play in mpv or IINA, without the site's own player, cookies or account. mpv needs yt-dlp. Leave empty to find a player automatically, or enter a program name or path.</span>
            </div>
            <input class="input mono" bind:value={videoPlayer} onblur={saveVideoPlayer} onkeydown={onEnter(saveVideoPlayer)} placeholder="Automatic" spellcheck="false" autocomplete="off" />
            <span class="hint">
              {#if store.videoPlayer}Using {store.videoPlayer.name}: <code class="selectable">{store.videoPlayer.path}</code>
              {:else if s.videoPlayer}This program was not found.
              {:else}No player found. Install mpv or IINA.{/if}
            </span>
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
      {:else if current === 'security'}
        <div class="card">
          {#each store.components ?? [] as c (c.id)}
            <div class="row stack">
              <div class="row-text">
                <span class="row-title">
                  {c.name}
                  {#if c.version}<span class="comp-version">{c.version}</span>{/if}
                  {#if STATUS[c.status]}<span class="comp-status {c.status}">{STATUS[c.status]}</span>{/if}
                </span>
                <span class="hint">{c.message}</span>
                {#if c.detail}<span class="hint comp-detail">{c.detail}</span>{/if}
              </div>
              {#if c.command || c.link}
                <div class="inline">
                  {#if c.command}
                    {@const command = c.command}
                    <code class="path selectable">{command}</code>
                    <button class="btn small" onclick={() => copyText(command).then(() => store.toast('Command copied'))}><Icon name="copy" size={13} />Copy</button>
                  {/if}
                  {#if c.link}
                    <button class="btn small" onclick={() => store.openExternal(c.link)}><Icon name="external" size={13} />Open page</button>
                  {/if}
                </div>
              {/if}
            </div>
          {:else}
            <div class="row"><span class="hint">Looking at the installed versions…</span></div>
          {/each}
        </div>
        <div class="card">
          <div class="row">
            <div class="row-text">
              <span class="row-title">Check online</span>
              <span class="hint">
                Asks GitHub for the newest Tidings and yt-dlp releases{isMac ? '' : ', and webkitgtk.org for its latest security advisory'},
                through your proxy. Tidings never does this by itself; without it, the list above only knows how old things are.
                {#if store.componentsCheckedOnline}Last checked at {new Date(store.componentsCheckedOnline).toLocaleTimeString()}.{/if}
              </span>
            </div>
            <button class="btn small" onclick={() => store.checkComponentsOnline()} disabled={store.checkingComponents}>
              {#if store.checkingComponents}<span class="spinner"></span>{:else}<Icon name="refresh" size={13} />{/if}Check now
            </button>
          </div>
        </div>
      {:else if current === 'ai'}
        <div class="card">
          <div class="row stack">
            <div class="row-text">
              <span class="row-title">Local model</span>
              <span class="hint">
                A brief summarizes the articles the list shows (a folder, a type, a date range) in a few topics, with links to
                its sources. It is written by a language model on this computer or your local network, never in the cloud.
                Tidings sends that model the titles, feed names, dates and short excerpts of those articles, and nothing else.
              </span>
            </div>
            <div class="ai-providers">
              {#each AI_PROVIDERS as p (p.id)}
                <label class="radio"><input type="radio" checked={s.aiProvider === p.id} onchange={() => setAiProvider(p.id)} /> {p.label}</label>
              {/each}
            </div>
          </div>
          {#if s.aiProvider}
            <div class="row stack">
              <div class="row-text">
                <span class="row-title">Server address</span>
                <span class="hint">Only this computer or your local network. Leave empty for {aiDefault}{s.aiProvider === 'openai' ? ' (LM Studio; llama.cpp’s llama-server uses port 8080)' : ''}.</span>
              </div>
              <input class="input mono" bind:value={aiEndpoint} onblur={saveAiEndpoint} onkeydown={onEnter(saveAiEndpoint)} placeholder={aiDefault} spellcheck="false" autocomplete="off" />
            </div>
            <div class="row stack">
              <div class="row-text">
                <span class="row-title">Model</span>
                <span class="hint">
                  {#if aiLoading}Asking the server for its models…
                  {:else if aiModelsError}{aiModelsError}
                  {:else if aiModels}The server is running. Small instruction models (3–8B, such as Qwen3 4B Instruct or Llama 3.2 3B) write briefs in about a minute.
                  {:else}Choose the model that writes briefs.{/if}
                </span>
              </div>
              <div class="inline">
                {#if aiModels && aiModels.length > 0}
                  <select class="input" value={aiModel} onchange={(e) => saveAiModel((e.currentTarget as HTMLSelectElement).value)}>
                    {#if !aiModels.includes(aiModel)}<option value={aiModel}>{aiModel || 'Choose a model…'}</option>{/if}
                    {#each aiModels as m (m)}<option value={m}>{m}</option>{/each}
                  </select>
                {:else}
                  <input class="input mono" bind:value={aiModel} onblur={() => saveAiModel(aiModel)} onkeydown={onEnter(() => saveAiModel(aiModel))} placeholder="qwen3:4b" spellcheck="false" autocomplete="off" />
                {/if}
                <button class="btn small" onclick={loadAiModels} disabled={aiLoading}>
                  {#if aiLoading}<span class="spinner"></span>{:else}<Icon name="refresh" size={13} />{/if}Check server
                </button>
              </div>
            </div>
          {/if}
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
          <div class="row">
            <div class="row-text">
              <span class="row-title">Start over</span>
              <span class="hint">Removes every feed and folder, with all articles. Settings stay.</span>
            </div>
            <button class="btn small danger" onclick={() => store.deleteAllSubscriptions()}><Icon name="trash" size={13} />Remove all…</button>
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
            <li><Icon name="check" size={14} />No telemetry, no analytics, no automatic update checks, no accounts</li>
            <li><Icon name="check" size={14} />Your data stays in a local database on this computer</li>
          </ul>
        </div>
      {/if}
    </div>
  </div>
</Modal>

<style>
  .ai-providers {
    display: grid;
    gap: 6px;
  }
  .comp-version {
    margin-left: 4px;
    font-weight: 400;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }
  .comp-status {
    margin-left: 6px;
    padding: 1px 7px;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 500;
    background: var(--bg-hover);
    color: var(--text-2);
  }
  .comp-status.ok {
    color: var(--success, #2f9e5b);
  }
  .comp-status.old,
  .comp-status.update {
    background: var(--accent-softer);
    color: var(--accent);
  }
  .comp-detail {
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 11px;
    word-break: break-all;
  }
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
