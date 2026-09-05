/**
 * A small fuzzy matcher for the command palette.
 *
 * Subsequence matching, case-insensitive: every character of the query must
 * appear in the text in order, but not necessarily adjacently. The score
 * rewards runs of adjacent matches and matches at the start of a word, so
 * "nsh" ranks "New SSH" above a host that merely contains n, s and h scattered.
 */

const WORD_BOUNDARY = /[\s/@:._\-]/;

/**
 * Scores how well `query` matches `text`, or `null` if it does not match at
 * all. Higher is better. An empty query matches everything with a flat score,
 * so the palette shows the full list before anything is typed.
 */
export function fuzzyScore(query: string, text: string): number | null {
  if (query === "") return 0;
  const q = query.toLowerCase();
  const t = text.toLowerCase();

  let qi = 0;
  let score = 0;
  let streak = 0;
  let prev = -2;
  for (let ti = 0; ti < t.length && qi < q.length; ti += 1) {
    if (t[ti] !== q[qi]) continue;
    // A run of adjacent matches is worth more the longer it runs.
    streak = ti === prev + 1 ? streak + 1 : 1;
    let bonus = streak;
    // The first letter of the text, or of a word within it, is a strong signal.
    if (ti === 0 || WORD_BOUNDARY.test(t[ti - 1])) bonus += 4;
    score += bonus;
    prev = ti;
    qi += 1;
  }

  if (qi < q.length) return null;
  // Among equal matches, prefer the shorter text (a tighter match).
  return score - t.length * 0.01;
}

/**
 * Filters and ranks `items` by how well `query` matches `key(item)`, best
 * first. Ties keep the original order, so a stable input stays stable.
 */
export function fuzzyRank<T>(query: string, items: T[], key: (item: T) => string): T[] {
  return items
    .map((item, index) => ({ item, index, score: fuzzyScore(query, key(item)) }))
    .filter((entry): entry is { item: T; index: number; score: number } => entry.score !== null)
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .map((entry) => entry.item);
}
