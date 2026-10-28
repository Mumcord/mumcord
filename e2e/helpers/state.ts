/**
 * Harness state that the wdio config and the spec files share.
 *
 * WebdriverIO 10 loads the two through different module registries: the worker re-imports
 * `e2e/wdio.conf.ts` for its hooks, while Mocha loads each spec file on its own. A module-level
 * variable in a helper then exists twice per worker, so `beforeSession` and the spec would not see
 * the same driver, profile or netsim proxy. Registering the state on `globalThis` gives both sides
 * one shared instance.
 */
export function shared<T>(key: string, initial: T): T {
  // globalThis has no index signature; the entry we write is exactly the type T we ask for.
  const registry = globalThis as unknown as Record<string, T>;
  const name = `__e2e_${key}`;
  if (!registry[name]) registry[name] = initial;
  return registry[name];
}
