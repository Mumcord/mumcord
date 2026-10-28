/**
 * XPath predicate for a class token. `contains(@class,'tree-person')` also matches a child with
 * `tree-person__name`, so a strict `$()` (wdio 10) counts several elements for one row. Matching the
 * whole class attribute as space-separated tokens keeps the selector on the element itself.
 */
export function hasClass(name: string): string {
  return `contains(concat(' ', normalize-space(@class), ' '), ' ${name} ')`;
}
