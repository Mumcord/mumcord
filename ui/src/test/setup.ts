import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

afterEach(() => {
  cleanup();
});

// jsdom has no layout, so it has no scrollIntoView either.
if (typeof Element !== "undefined") Element.prototype.scrollIntoView = () => undefined;
