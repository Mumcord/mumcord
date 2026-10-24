/** Selector helpers for `useSyncExternalStore`-based hooks (server store, tree rows). */

/**
 * A snapshot function that returns its previous result while `equal` says nothing changed, so
 * a component selecting a derived value (e.g. a new array) re-renders only on real changes.
 */
export function cachedSelector<S, T>(
  getState: () => S,
  select: (state: S) => T,
  equal: (a: T, b: T) => boolean,
): () => T {
  let last: { value: T } | null = null;
  return () => {
    const value = select(getState());
    if (last !== null && equal(last.value, value)) return last.value;
    last = { value };
    return value;
  };
}

/** `Object.is` for single values, item-wise `Object.is` for arrays. */
export function sameValue<T>(a: T, b: T): boolean {
  if (Object.is(a, b)) return true;
  if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) return false;
  return a.every((item, i) => Object.is(item, b[i]));
}
