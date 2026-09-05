/**
 * Consolidated time formatters (single source of truth).
 * `now` is injectable for testability (defaults to Date.now()).
 */

export function formatRelativeTime(
  value: string | number | Date,
  now: number = Date.now(),
): string {
  const timestamp = value instanceof Date ? value.getTime() : new Date(value).getTime();
  if (!Number.isFinite(timestamp)) return "";
  const days = Math.floor((now - timestamp) / 86_400_000);
  if (days <= 0) return "Today";
  if (days === 1) return "Yesterday";
  if (days < 7) return `${days} days ago`;
  return new Date(timestamp).toLocaleDateString();
}

export function formatClock(date: Date, use24Hour = false): string {
  return date.toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: !use24Hour,
  });
}

export function formatDate(value: string | number | Date): string {
  const date = value instanceof Date ? value : new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleDateString();
}

const timeFormatter = new Intl.DateTimeFormat("en-US", {
  hour: "numeric",
  minute: "2-digit",
  second: "2-digit",
  hour12: true,
});

export function formatTime(timestamp: number | string): string {
  const time = typeof timestamp === "number" ? timestamp : new Date(timestamp).getTime();
  if (!Number.isFinite(time)) return "";
  return timeFormatter.format(time);
}

/** @deprecated Use formatRelativeTime instead. */
export function relativeDate(value: string, now: number = Date.now()): string {
  return formatRelativeTime(value, now);
}
