// Feed types and date ranges: the two ways to narrow the article list besides
// folders and read state.
import type { DateRange, FeedType } from './types';

export interface TypeInfo {
  value: FeedType;
  /** Plural, for the sidebar and list title. */
  label: string;
  /** Singular, for a single feed. */
  one: string;
  icon: string;
}

export const FEED_TYPES: TypeInfo[] = [
  { value: 'podcast', label: 'Podcasts', one: 'Podcast', icon: 'headphones' },
  { value: 'video', label: 'Videos', one: 'Video', icon: 'video' },
  { value: 'code', label: 'Code & releases', one: 'Code & releases', icon: 'code' },
  { value: 'discussion', label: 'Discussions', one: 'Discussion', icon: 'message' },
  { value: 'newsletter', label: 'Newsletters', one: 'Newsletter', icon: 'mail' },
  { value: 'article', label: 'Articles', one: 'Article', icon: 'file-text' },
];

export function typeInfo(type: FeedType): TypeInfo {
  return FEED_TYPES.find((t) => t.value === type) ?? FEED_TYPES[FEED_TYPES.length - 1];
}

type Preset = DateRange['preset'];

export const DATE_PRESETS: { value: Exclude<Preset, 'custom'>; label: string }[] = [
  { value: 'any', label: 'Any time' },
  { value: 'today', label: 'Today' },
  { value: 'yesterday', label: 'Yesterday' },
  { value: 'week', label: 'Last 7 days' },
  { value: 'month', label: 'Last 30 days' },
  { value: 'year', label: 'This year' },
  { value: 'older', label: 'Older than 30 days' },
];

/** Local midnight, `days` days after `d` (calendar days, so DST changes do not shift it). */
function dayStart(d: Date, days = 0): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + days);
}

/** `YYYY-MM-DD` as local midnight. */
export function parseDay(value: string | null | undefined): Date | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value ?? '');
  if (!m) return null;
  const d = new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]));
  return Number.isNaN(d.getTime()) ? null : d;
}

/** Local date as `YYYY-MM-DD`, for date inputs. */
export function formatDay(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/**
 * The range as Unix seconds: published at or after `since`, before `until`.
 * Relative presets are worked out from `now`, so they stay right across midnight.
 */
export function rangeBounds(range: DateRange, now = new Date()): { since: number | null; until: number | null } {
  const sec = (d: Date | null) => (d ? Math.floor(d.getTime() / 1000) : null);
  const today = dayStart(now);
  switch (range.preset) {
    case 'today':
      return { since: sec(today), until: null };
    case 'yesterday':
      return { since: sec(dayStart(today, -1)), until: sec(today) };
    case 'week':
      return { since: sec(dayStart(today, -6)), until: null };
    case 'month':
      return { since: sec(dayStart(today, -29)), until: null };
    case 'year':
      return { since: sec(new Date(now.getFullYear(), 0, 1)), until: null };
    case 'older':
      // Exactly what "Last 30 days" leaves out.
      return { since: null, until: sec(dayStart(today, -29)) };
    case 'custom': {
      const from = parseDay(range.from);
      const to = parseDay(range.to);
      return { since: sec(from), until: sec(to ? dayStart(to, 1) : null) };
    }
    default:
      return { since: null, until: null };
  }
}

export function isAnyTime(range: DateRange): boolean {
  if (range.preset === 'custom') return !parseDay(range.from) && !parseDay(range.to);
  return range.preset === 'any';
}

export function rangeLabel(range: DateRange): string {
  if (range.preset !== 'custom') return DATE_PRESETS.find((p) => p.value === range.preset)?.label ?? 'Any time';
  const from = parseDay(range.from);
  const to = parseDay(range.to);
  const thisYear = new Date().getFullYear();
  const short = (d: Date) => d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  const long = (d: Date) => d.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
  // The year only when it is not this year, and only once for a range within one year.
  const fmt = (d: Date) => (d.getFullYear() === thisYear ? short(d) : long(d));
  if (from && to) {
    if (from.getTime() === to.getTime()) return fmt(from);
    if (from.getFullYear() !== to.getFullYear()) return `${long(from)} – ${long(to)}`;
    return to.getFullYear() === thisYear ? `${short(from)} – ${short(to)}` : `${short(from)} – ${long(to)}`;
  }
  if (from) return `Since ${fmt(from)}`;
  if (to) return `Until ${fmt(to)}`;
  return 'Any time';
}

/** Restores a saved range, falling back to "Any time" for anything unexpected. */
export function validRange(value: unknown): DateRange {
  const v = value as Partial<{ preset: string; from: unknown; to: unknown }> | null;
  if (v && DATE_PRESETS.some((p) => p.value === v.preset)) return { preset: v.preset as Exclude<Preset, 'custom'> };
  if (v?.preset === 'custom') {
    const day = (x: unknown) => (typeof x === 'string' && parseDay(x) ? x : null);
    return { preset: 'custom', from: day(v.from), to: day(v.to) };
  }
  return { preset: 'any' };
}
