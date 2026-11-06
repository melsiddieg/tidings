// Drag and drop state for the subscription tree.
import type { ItemRef } from './types';

export type DragItem = ItemRef;
export type Zone = 'before' | 'into' | 'after';

export const drag = $state<{ item: DragItem | null; over: string | null }>({ item: null, over: null });

export function startDrag(e: DragEvent, item: DragItem) {
  drag.item = item;
  e.dataTransfer?.setData('text/plain', `${item.kind}:${item.id}`);
  if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move';
}

export function endDrag() {
  drag.item = null;
  drag.over = null;
}

/** Which part of a row the pointer is over. Rows that cannot hold items only have before/after. */
export function zoneOf(e: DragEvent, canHold: boolean): Zone {
  const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
  const y = (e.clientY - rect.top) / rect.height;
  if (!canHold) return y < 0.5 ? 'before' : 'after';
  if (y < 0.28) return 'before';
  if (y > 0.72) return 'after';
  return 'into';
}

/** The zone currently shown on the row with `key`, if any. */
export function zoneFor(key: string): Zone | null {
  return drag.over?.startsWith(`${key}:`) ? (drag.over.slice(key.length + 1) as Zone) : null;
}
