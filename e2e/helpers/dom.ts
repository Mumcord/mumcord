import { browser } from "@wdio/globals";

/** Checks CSS or absolute XPath selector absence without handles to nodes being unmounted. */
export async function waitForAbsent(
  selector: string,
  options: { timeout: number; timeoutMsg: string },
): Promise<void> {
  // WebKit may throw a stale-element error while creating a handle, even on a fresh lookup.
  // Return only a boolean from the page so disappearing nodes never cross WebDriver's boundary.
  await browser.waitUntil(
    () =>
      browser.execute((query: string) => {
        const node = query.startsWith("/")
          ? document.evaluate(query, document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null)
              .singleNodeValue
          : document.querySelector(query);
        return node === null;
      }, selector),
    options,
  );
}
