import axe from "axe-core";

/** Runs axe on a rendered container and returns the rule ids of all violations. */
export async function axeViolations(container: Element): Promise<string[]> {
  const results = await axe.run(container);
  return results.violations.map((violation) => violation.id);
}
