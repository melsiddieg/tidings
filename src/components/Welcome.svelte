<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { api } from '../lib/api';
  import { store } from '../lib/store.svelte';

  // Suggestions only. Nothing is contacted until the user clicks "Add".
  const STARTERS = [
    { title: 'Hacker News', url: 'https://news.ycombinator.com/rss', about: 'Technology and startups' },
    { title: 'Ars Technica', url: 'https://feeds.arstechnica.com/arstechnica/index', about: 'Tech, science and policy' },
    { title: 'BBC News · World', url: 'https://feeds.bbci.co.uk/news/world/rss.xml', about: 'International news' },
    { title: 'Al Jazeera', url: 'https://www.aljazeera.com/xml/rss/all.xml', about: 'World news' },
    { title: 'LWN.net', url: 'https://lwn.net/headlines/rss', about: 'Linux and free software' },
    { title: 'Planet KDE', url: 'https://planet.kde.org/global/atom.xml', about: 'KDE community blogs' },
    { title: 'Quanta Magazine', url: 'https://www.quantamagazine.org/feed/', about: 'Science and mathematics' },
    { title: 'The Rust Blog', url: 'https://blog.rust-lang.org/feed.xml', about: 'Rust language news' },
  ];

  let added = $state<Record<string, boolean>>({});
  let busy = $state<Record<string, boolean>>({});

  async function add(s: (typeof STARTERS)[number]) {
    busy[s.url] = true;
    try {
      const id = await api.addFeed(s.url, null, s.title);
      store.watchFeed(id);
      added[s.url] = true;
      await store.reloadTree();
    } catch (e) {
      store.error(e);
    } finally {
      busy[s.url] = false;
    }
  }
</script>

<div class="welcome">
  <div class="inner">
    <div class="hero">
      <div class="mark"><Icon name="rss" size={30} stroke={2.4} /></div>
      <h1>Welcome to Tidings</h1>
      <p>A calm, private place for the news you follow. Your subscriptions and reading history stay on this computer.</p>
      <div class="cta">
        <button class="btn primary" onclick={() => (store.dialog = { kind: 'add-feed', folderId: null })}>
          <Icon name="plus" size={15} />Add a feed
        </button>
        <button class="btn" onclick={() => store.importOpml()}>
          <Icon name="download" size={15} />Import OPML
        </button>
      </div>
    </div>

    <h2>Or start with a few of these</h2>
    <div class="grid">
      {#each STARTERS as s (s.url)}
        <div class="starter">
          <div class="starter-text">
            <strong>{s.title}</strong>
            <span>{s.about}</span>
          </div>
          {#if added[s.url]}
            <span class="done"><Icon name="check" size={15} stroke={2.4} /></span>
          {:else}
            <button class="btn small" onclick={() => add(s)} disabled={busy[s.url]}>
              {#if busy[s.url]}<span class="spinner"></span>{:else}Add{/if}
            </button>
          {/if}
        </div>
      {/each}
    </div>

    <div class="privacy">
      <Icon name="shield" size={16} />
      <span>No accounts, no telemetry, no tracking. Tidings only connects to the feeds you add and the pages you open.</span>
    </div>
  </div>
</div>

<style>
  .welcome {
    overflow-y: auto;
    min-width: 0;
    background:
      radial-gradient(1200px 500px at 50% -10%, var(--accent-softer), transparent 60%),
      var(--bg-content);
  }
  .inner {
    max-width: 640px;
    margin: 0 auto;
    padding: 72px 32px 48px;
    animation: fade-up 0.4s var(--ease-out);
  }
  .hero {
    text-align: center;
    display: grid;
    justify-items: center;
    gap: 12px;
  }
  .mark {
    display: grid;
    place-items: center;
    width: 72px;
    height: 72px;
    border-radius: 20px;
    color: #fff;
    background: linear-gradient(145deg, #ffa061, #f4683c 55%, #d2402e);
    box-shadow: 0 12px 30px -8px rgba(210, 64, 46, 0.55), inset 0 1px 0 rgba(255, 255, 255, 0.3);
    margin-bottom: 8px;
  }
  h1 {
    margin: 0;
    font-size: 28px;
    letter-spacing: -0.025em;
  }
  .hero p {
    margin: 0;
    max-width: 440px;
    color: var(--text-2);
    font-size: 14px;
    line-height: 1.55;
  }
  .cta {
    display: flex;
    gap: 8px;
    margin-top: 10px;
  }
  .cta .btn {
    height: 36px;
    padding: 0 16px;
  }
  h2 {
    margin: 52px 0 12px;
    font-size: 11.5px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-3);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 8px;
  }
  .starter {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 12px 12px 14px;
    border-radius: 12px;
    background: var(--bg-elevated);
    box-shadow: var(--shadow-sm);
  }
  .starter-text {
    flex: 1;
    display: grid;
    min-width: 0;
  }
  .starter-text strong {
    font-size: 13px;
  }
  .starter-text span {
    font-size: 12px;
    color: var(--text-3);
  }
  .done {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    color: var(--success);
    background: color-mix(in srgb, var(--success) 14%, transparent);
  }
  .privacy {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 36px;
    padding: 14px 16px;
    border-radius: 12px;
    background: var(--bg-muted);
    color: var(--text-2);
    font-size: 12.5px;
  }
  .privacy :global(svg) {
    color: var(--success);
  }
</style>
