export type SearchTextPart = { text: string; match: boolean };

export function highlightSearchText(value: string, query: string): SearchTextPart[] {
  const normalizedQuery = query.trim();
  if (!normalizedQuery) return [{ text: value, match: false }];
  const lowerValue = value.toLocaleLowerCase();
  const lowerQuery = normalizedQuery.toLocaleLowerCase();
  const parts: SearchTextPart[] = [];
  let cursor = 0;
  while (cursor < value.length) {
    const index = lowerValue.indexOf(lowerQuery, cursor);
    if (index < 0) {
      if (cursor < value.length) parts.push({ text: value.slice(cursor), match: false });
      break;
    }
    if (index > cursor) parts.push({ text: value.slice(cursor, index), match: false });
    parts.push({ text: value.slice(index, index + normalizedQuery.length), match: true });
    cursor = index + normalizedQuery.length;
  }
  return parts;
}
