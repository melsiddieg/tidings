// Drag and drop state for the subscription tree.
export type DragItem = { type: 'feed' | 'folder'; id: number };

export const drag = $state<{ item: DragItem | null; over: string | null }>({ item: null, over: null });

export function startDrag(e: DragEvent, item: DragItem) {
  drag.item = item;
  e.dataTransfer?.setData('text/plain', `${item.type}:${item.id}`);
  if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move';
}

export function endDrag() {
  drag.item = null;
  drag.over = null;
}
