<script lang="ts">
  // A brief written by a local AI model (brief.rs): topics with short points, each
  // citing the articles it comes from. The model's text is shown as plain text.
  import Icon from '../lib/Icon.svelte';
  import FeedIcon from './FeedIcon.svelte';
  import { api, errorText } from '../lib/api';
  import { store } from '../lib/store.svelte';
  import type { Brief, BriefSource, Tab } from '../lib/types';
  import { longTime } from '../lib/util';

  let { tab }: { tab: Extract<Tab, { kind: 'brief' }> } = $props();

  let brief = $state<Brief | null>(null);
  let loadError = $state<string | null>(null);
  let elapsed = $state(0);

  const running = $derived(store.briefsRunning.includes(tab.id));
  const error = $derived(store.briefErrors[tab.id] ?? loadError);
  const byId = $derived(new Map((brief?.sources ?? []).map((s) => [s.id, s])));

  // Load the stored brief whenever the tab points at another one.
  $effect(() => {
    const id = tab.briefId;
    if (id == null) {
      brief = null;
      return;
    }
    loadError = null;
    api
      .brief(id)
      .then((b) => {
        if (tab.briefId !== id) return;
        brief = b;
        if (!b) loadError = 'This brief was deleted.';
      })
      .catch((e) => (loadError = errorText(e)));
  });

  // Seconds since the model started, while it writes.
  $effect(() => {
    if (!running) return;
    elapsed = 0;
    const timer = setInterval(() => elapsed++, 1000);
    return () => clearInterval(timer);
  });

  function sourcesOf(ids: number[]): BriefSource[] {
    return ids.map((id) => byId.get(id)).filter((s): s is BriefSource => !!s);
  }

  function openSource(s: BriefSource) {
    store.openArticleTab(s.id, false, { title: s.title, feedId: s.feedId });
  }

  async function history(e: MouseEvent) {
    const target = e.currentTarget as HTMLElement;
    try {
      const list = await api.briefs();
      store.openMenuAt(
        target,
        list.length
          ? list.slice(0, 20).map((b) => ({
              label: `${b.title} · ${longTime(b.createdAt)}`,
              icon: 'sparkles',
              checked: b.id === tab.briefId,
              action: () => store.openBrief(b),
            }))
          : [{ label: 'No briefs yet', disabled: true, action: () => {} }],
      );
    } catch (err) {
      store.error(err);
    }
  }

  function remove() {
    if (!brief) return;
    const id = brief.id;
    store.confirm('Delete this brief?', 'It is removed from this computer. The articles stay.', 'Delete', () => store.deleteBrief(tab.id, id));
  }
</script>

<div class="article-frame">
  <div class="article-toolbar">
    <div class="group">
      <button class="icon-btn" title="Write this brief again" onclick={() => brief && store.rewriteBrief(tab.id, brief.query)} disabled={running || !brief}>
        <Icon name="refresh" size={15} />
      </button>
      <button class="icon-btn" title="Earlier briefs" onclick={history}>
        <Icon name="clock" size={15} />
      </button>
      <span class="sep"></span>
      <span class="brief-pill"><Icon name="lock" size={13} /> Written on this computer by {brief?.model ?? store.settings.aiModel}</span>
    </div>
    <div class="group">
      <button class="icon-btn" title="Delete this brief" onclick={remove} disabled={!brief || running}>
        <Icon name="trash" size={15} />
      </button>
    </div>
  </div>

  <div class="article-scroll">
    {#if running}
      <div class="article-state">
        <span class="spinner"></span>
        <p>Writing a brief of <strong>{tab.title}</strong> with {store.settings.aiModel}… {elapsed} s</p>
        <p class="hint">A local model can take a minute or two. You can keep reading in another tab.</p>
      </div>
    {:else if error}
      <div class="article-state">
        <div class="error-icon"><Icon name="sparkles" size={24} stroke={1.5} /></div>
        <h3>The brief could not be written</h3>
        <p>{error}</p>
        <div class="actions">
          {#if tab.query}<button class="btn primary" onclick={() => store.runBrief(tab.id)}>Try again</button>{/if}
          <button class="btn" onclick={() => (store.dialog = { kind: 'settings', section: 'ai' })}><Icon name="settings" size={14} />AI settings</button>
        </div>
      </div>
    {:else if brief}
      <article class="article wide">
        <header class="article-head">
          <div class="kicker">
            <Icon name="sparkles" size={14} />
            <span>Brief</span>
            <span class="dot-sep">·</span>
            <time>{longTime(brief.createdAt)}</time>
          </div>
          <h1 class="article-title" dir="auto">{brief.title}</h1>
          <div class="byline">
            From {brief.articlesUsed === brief.articlesTotal ? brief.articlesTotal : `the newest ${brief.articlesUsed} of ${brief.articlesTotal}`}
            article{brief.articlesTotal === 1 ? '' : 's'} · {brief.model} · {brief.seconds} s
          </div>
        </header>

        {#each brief.sections as section, i (i)}
          <section class="brief-section">
            {#if section.title}<h2 dir="auto">{section.title}</h2>{/if}
            <ul>
              {#each section.points as point, j (j)}
                <li>
                  <span dir="auto">{point.text}</span>
                  {#each sourcesOf(point.sources) as s (s.id)}
                    <button class="source" title={s.title} onclick={() => openSource(s)}>
                      <FeedIcon feed={store.feedById.get(s.feedId)} size={12} />{s.feed}
                    </button>
                  {/each}
                </li>
              {/each}
            </ul>
          </section>
        {/each}

        <footer class="brief-foot">
          <Icon name="alert" size={14} />
          <span>Written by a language model from headlines and short excerpts. It can be wrong or leave things out: open the sources before you rely on it.</span>
        </footer>
      </article>
    {:else}
      <div class="article-state"><span class="spinner"></span></div>
    {/if}
  </div>
</div>

<style>
  .brief-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-3);
  }
  .brief-section h2 {
    font-size: 1.15em;
    margin: 1.4em 0 0.4em;
  }
  .brief-section ul {
    margin: 0;
    padding-left: 1.2em;
  }
  .brief-section li {
    margin: 0.45em 0;
    line-height: 1.55;
  }
  .source {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin-left: 6px;
    padding: 1px 7px 1px 5px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--bg-muted);
    color: var(--text-2);
    font-size: 0.72em;
    vertical-align: 0.1em;
    white-space: nowrap;
  }
  .source:hover {
    background: var(--bg-hover);
    color: var(--text);
  }
  .brief-foot {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    margin-top: 2em;
    padding-top: 1em;
    border-top: 1px solid var(--border);
    font-size: 13px;
    color: var(--text-3);
  }
</style>
