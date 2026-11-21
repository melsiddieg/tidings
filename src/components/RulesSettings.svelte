<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { api, errorText } from '../lib/api';
  import { store } from '../lib/store.svelte';
  import type { Rule } from '../lib/types';

  const FIELDS: { id: Rule['field']; label: string }[] = [
    { id: 'title', label: 'Title' },
    { id: 'text', label: 'Title or text' },
    { id: 'author', label: 'Author' },
    { id: 'link', label: 'Link' },
  ];
  const ACTIONS: { id: Rule['action']; label: string; done: string }[] = [
    { id: 'read', label: 'Mark as read', done: 'marked as read' },
    { id: 'star', label: 'Star', done: 'starred' },
    { id: 'delete', label: 'Delete', done: 'deleted' },
  ];

  let rules = $state<Rule[]>([]);
  let loadError = $state<string | null>(null);
  /** The rule being edited; id 0 is a new one. */
  let draft = $state<Rule | null>(null);
  let draftError = $state<string | null>(null);
  /** Stored articles the draft would change now (null while counting). */
  let matches = $state<number | null>(null);
  let confirming = $state(false);
  let busy = $state(false);

  async function load() {
    try {
      rules = await api.rules();
      loadError = null;
    } catch (e) {
      loadError = errorText(e);
    }
  }
  load();

  const blank = (): Rule => ({ id: 0, enabled: true, feedId: null, folderId: null, field: 'title', words: '', action: 'read', hits: 0 });

  // Folders (indented by depth) and feeds, for "in".
  const folderOptions = $derived.by(() => {
    const out: { id: number; label: string }[] = [];
    const walk = (parent: number | null, depth: number) => {
      for (const f of store.tree.folders.filter((x) => x.parentId === parent).sort((a, b) => a.position - b.position)) {
        // Menus drop ordinary leading spaces: indent with no-break spaces.
        out.push({ id: f.id, label: `${'\u00a0\u00a0\u00a0'.repeat(depth)}${f.title}` });
        walk(f.id, depth + 1);
      }
    };
    walk(null, 0);
    return out;
  });
  const feedOptions = $derived([...store.tree.feeds].sort((a, b) => a.title.localeCompare(b.title)));

  const scopeValue = (r: Rule) => (r.feedId != null ? `feed:${r.feedId}` : r.folderId != null ? `folder:${r.folderId}` : 'all');
  function setScope(r: Rule, value: string) {
    const [kind, id] = value.split(':');
    r.feedId = kind === 'feed' ? Number(id) : null;
    r.folderId = kind === 'folder' ? Number(id) : null;
  }
  function scopeLabel(r: Rule): string {
    if (r.feedId != null) return store.feedById.get(r.feedId)?.title ?? 'a feed';
    if (r.folderId != null) return `the ${store.folderById.get(r.folderId)?.title ?? ''} folder`;
    return 'all feeds';
  }
  const fieldLabel = (r: Rule) => FIELDS.find((f) => f.id === r.field)?.label ?? r.field;
  const action = (r: Rule) => ACTIONS.find((a) => a.id === r.action) ?? ACTIONS[0];

  function edit(rule: Rule | null) {
    draft = rule ? { ...rule } : blank();
    draftError = null;
    confirming = false;
  }

  // How many stored articles the draft matches, a moment after it changes.
  let counting = 0;
  $effect(() => {
    const d = draft ? $state.snapshot(draft) : null;
    const run = ++counting;
    matches = null;
    confirming = false;
    if (!d || !d.words.trim()) return;
    const timer = setTimeout(async () => {
      try {
        const n = await api.countRuleMatches(d);
        if (run === counting) matches = n;
      } catch {
        // The draft is not complete yet; saving shows why.
      }
    }, 400);
    return () => clearTimeout(timer);
  });

  /** Saves the draft; with `apply`, also does its action to the articles it matches now. */
  async function save(apply: boolean) {
    if (!draft || busy) return;
    if (apply && draft.action === 'delete' && !confirming) {
      confirming = true;
      return;
    }
    busy = true;
    try {
      const saved = await api.saveRule($state.snapshot(draft));
      let note = 'Rule saved';
      if (apply) {
        const n = await api.applyRule(saved.id);
        note = `Rule saved. ${n} article${n === 1 ? '' : 's'} ${action(saved).done}.`;
        await store.reloadTree();
        await store.loadArticles();
      }
      draft = null;
      await load();
      store.toast(note);
    } catch (e) {
      draftError = errorText(e);
    } finally {
      busy = false;
      confirming = false;
    }
  }

  async function toggle(rule: Rule) {
    try {
      await api.saveRule({ ...$state.snapshot(rule), enabled: !rule.enabled });
      await load();
    } catch (e) {
      store.error(e);
    }
  }

  async function remove(rule: Rule) {
    try {
      await api.deleteRule(rule.id);
      if (draft?.id === rule.id) draft = null;
      await load();
    } catch (e) {
      store.error(e);
    }
  }
</script>

<div class="card">
  <div class="row stack">
    <div class="row-text">
      <span class="row-title">Filter rules</span>
      <span class="hint">
        Rules run on new articles when feeds refresh. Separate several words or phrases with commas: one is enough. Words match
        whole, in any case (“AI” does not match “said”); end one with * to match the start of a word (“crypto*”). Starred
        articles are never deleted or marked as read by a rule.
      </span>
    </div>
  </div>

  {#if loadError}
    <div class="row"><span class="hint error">{loadError}</span></div>
  {/if}

  {#each rules as rule (rule.id)}
    <div class="row rule" class:off={!rule.enabled}>
      <button
        class="switch"
        class:on={rule.enabled}
        aria-pressed={rule.enabled}
        aria-label={rule.enabled ? 'Turn this rule off' : 'Turn this rule on'}
        title={rule.enabled ? 'Turn this rule off' : 'Turn this rule on'}
        onclick={() => toggle(rule)}
      ></button>
      <div class="row-text grow">
        <span class="sentence">
          {fieldLabel(rule)} contains <strong>{rule.words}</strong> in {scopeLabel(rule)}: <span class:deletes={rule.action === 'delete'}>{action(rule).label.toLowerCase()}</span>
        </span>
        <span class="hint">{rule.hits === 0 ? 'No matches yet' : `${rule.hits} article${rule.hits === 1 ? '' : 's'} so far`}</span>
      </div>
      <button class="icon-btn" title="Edit" onclick={() => edit(rule)}><Icon name="pencil" size={14} /></button>
      <button class="icon-btn" title="Delete this rule" onclick={() => remove(rule)}><Icon name="trash" size={14} /></button>
    </div>
  {/each}

  {#if draft}
    {@const d = draft}
    <div class="row stack editor">
      <div class="sentence-form">
        <select class="input compact" bind:value={d.field} aria-label="What the rule looks at">
          {#each FIELDS as f (f.id)}<option value={f.id}>{f.label}</option>{/each}
        </select>
        <span>contains</span>
        <input class="input words" bind:value={d.words} placeholder="bitcoin, crypto*, 广告" spellcheck="false" autocomplete="off" aria-label="Words" />
        <span>in</span>
        <select class="input compact" value={scopeValue(d)} onchange={(e) => setScope(d, e.currentTarget.value)} aria-label="Feeds">
          <option value="all">All feeds</option>
          {#if folderOptions.length}
            <optgroup label="Folder (with the folders inside it)">
              {#each folderOptions as f (f.id)}<option value={`folder:${f.id}`}>{f.label}</option>{/each}
            </optgroup>
          {/if}
          <optgroup label="Feed">
            {#each feedOptions as f (f.id)}<option value={`feed:${f.id}`}>{f.title}</option>{/each}
          </optgroup>
        </select>
        <span>then</span>
        <select class="input compact" bind:value={d.action} aria-label="What the rule does">
          {#each ACTIONS as a (a.id)}<option value={a.id}>{a.label}</option>{/each}
        </select>
      </div>
      {#if draftError}<span class="hint error">{draftError}</span>{/if}
      {#if confirming}
        <div class="confirm">
          <Icon name="alert" size={14} />
          <span>Delete {matches} article{matches === 1 ? '' : 's'} now? Their text is erased; this cannot be undone.</span>
        </div>
      {/if}
      <div class="inline">
        <button class="btn small primary" onclick={() => save(false)} disabled={busy || !d.words.trim()}>Save</button>
        {#if matches}
          <button class="btn small" class:danger={d.action === 'delete'} onclick={() => save(true)} disabled={busy}>
            {#if confirming}Delete {matches} articles{:else}Save and apply to the {matches} article{matches === 1 ? '' : 's'} stored now{/if}
          </button>
        {:else if matches === 0}
          <span class="hint">No stored article matches now.</span>
        {/if}
        <button class="btn small ghost" onclick={() => (confirming ? (confirming = false) : (draft = null))} disabled={busy}>Cancel</button>
      </div>
    </div>
  {:else}
    <div class="row">
      <button class="btn small" onclick={() => edit(null)}><Icon name="plus" size={13} />New rule</button>
    </div>
  {/if}
</div>

<style>
  .card {
    border-radius: 12px;
    border: 1px solid var(--border);
    background: var(--bg-content);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 14px;
  }
  .row + .row {
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
  .grow {
    flex: 1;
  }
  .row-title {
    font-size: 13px;
    font-weight: 500;
  }
  .sentence {
    font-size: 13px;
    overflow-wrap: anywhere;
  }
  .rule.off .sentence {
    color: var(--text-3);
  }
  .deletes {
    color: var(--danger, #d14);
  }
  .error {
    color: var(--danger, #d14);
  }
  .sentence-form {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }
  .compact {
    width: auto;
    height: 30px;
    flex: none;
  }
  .words {
    flex: 1 1 180px;
    min-width: 140px;
    height: 30px;
  }
  .inline {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
  }
  .confirm {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 12.5px;
    color: var(--danger, #d14);
  }
  .switch {
    flex: none;
  }
</style>
