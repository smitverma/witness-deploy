import { writable, type Readable } from "svelte/store";

export type ToastItem = {
  id: number;
  message: string;
};

export type CreateToastStoreOptions = {
  max?: number;
  durationMs?: number;
};

export type ToastStore<T extends ToastItem = ToastItem> = {
  subscribe: Readable<T[]>["subscribe"];
  show: (message: string, extra?: Omit<T, "id" | "message">) => T | undefined;
  dismiss: (id?: number) => void;
  clear: () => void;
};

export const TOAST_MAX = 3;
export const TOAST_DURATION_MS = 5_200;

export function toastMessage(reason: unknown): string {
  if (reason instanceof Error) return reason.message;
  if (
    reason &&
    typeof reason === "object" &&
    "message" in reason &&
    typeof (reason as { message: unknown }).message === "string"
  ) {
    return (reason as { message: string }).message;
  }
  return String(reason);
}

/**
 * Generic queued toast store.
 * - Keeps an array of toasts (queue), capped at `max` (default 3).
 * - Auto-dismisses each toast after `durationMs` (default 5200ms).
 * - Dedups identical messages already in the queue.
 */
export function createToastStore<T extends ToastItem = ToastItem>(
  options: CreateToastStoreOptions = {},
): ToastStore<T> {
  const max = options.max ?? TOAST_MAX;
  const durationMs = options.durationMs ?? TOAST_DURATION_MS;
  const store = writable<T[]>([]);
  let nextId = 1;
  const timers = new Map<number, ReturnType<typeof setTimeout>>();

  function clearTimer(id: number) {
    const timer = timers.get(id);
    if (timer) {
      clearTimeout(timer);
      timers.delete(id);
    }
  }

  function dismiss(id?: number) {
    if (id === undefined) {
      for (const timerId of [...timers.keys()]) clearTimer(timerId);
      store.set([]);
      return;
    }
    clearTimer(id);
    store.update((list) => list.filter((toast) => toast.id !== id));
  }

  function clear() {
    dismiss();
  }

  function show(message: string, extra?: Omit<T, "id" | "message">): T | undefined {
    const trimmed = message.trim();
    if (!trimmed) return undefined;
    let result: T | undefined;
    store.update((list) => {
      const existing = list.find((toast) => toast.message === trimmed);
      if (existing) {
        result = existing;
        return list;
      }
      const toast = { ...(extra as object | undefined), id: nextId++, message: trimmed } as T;
      result = toast;
      const next = [...list, toast].slice(-max);
      const evicted = list.filter((item) => !next.includes(item));
      for (const item of evicted) clearTimer(item.id);
      const id = toast.id;
      // Refresh timer for the new toast.
      clearTimer(id);
      timers.set(
        id,
        setTimeout(() => dismiss(id), durationMs),
      );
      return next;
    });
    return result;
  }

  return {
    subscribe: store.subscribe,
    show,
    dismiss,
    clear,
  };
}
