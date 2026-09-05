import { createToastStore, type ToastItem } from "./toast";

export type InfoToast = ToastItem;

const store = createToastStore<InfoToast>();

/** Queued info toasts (max 3). */
export const infoToasts = {
  subscribe: store.subscribe,
};

/**
 * Compat single-toast view: the most recent info toast, or null.
 * Kept so existing `$infoToast` usages keep working.
 */
export const infoToast = {
  subscribe(run: (value: InfoToast | null) => void) {
    return store.subscribe((list) => run(list.length ? list[list.length - 1] : null));
  },
};

export function showInfoToast(message: string) {
  const trimmed = message.trim();
  if (!trimmed) return;
  store.show(trimmed);
}

export function dismissInfoToast(id?: number) {
  store.dismiss(id);
}

export function clearInfoToasts() {
  store.clear();
}
