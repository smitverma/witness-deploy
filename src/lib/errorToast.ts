import { createToastStore, toastMessage, type ToastItem } from "./toast";

export type ErrorToast = ToastItem;

const store = createToastStore<ErrorToast>();

/** Queued error toasts (max 3). */
export const errorToasts = {
  subscribe: store.subscribe,
};

/**
 * Compat single-toast view: the most recent error, or null.
 * Kept so existing `$errorToast` usages keep working.
 */
export const errorToast = {
  subscribe(run: (value: ErrorToast | null) => void) {
    return store.subscribe((list) => run(list.length ? list[list.length - 1] : null));
  },
};

export function showErrorToast(reason: unknown) {
  const message = toastMessage(reason).trim();
  if (!message) return;
  store.show(message);
}

export function dismissErrorToast(id?: number) {
  store.dismiss(id);
}

export function clearErrorToasts() {
  store.clear();
}
