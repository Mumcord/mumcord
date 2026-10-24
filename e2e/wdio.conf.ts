import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { browser } from "@wdio/globals";
import { driverPort, prepareFresh, stopDriver } from "./helpers/app.ts";
import { netsim } from "./helpers/index.ts";
import { artifactsDir, bin } from "./helpers/paths.ts";

// Each spec file = one worker = one fresh install (new profile, empty keyring). Specs run one at a
// time because they share the test servers, the keyring and the netsim ports.
export const config: WebdriverIO.Config = {
  runner: "local",
  hostname: "127.0.0.1",
  port: driverPort,
  specs: ["./specs/**/*.e2e.ts"],
  maxInstances: 1,
  capabilities: [
    {
      // tauri-driver launches this binary through WebKitWebDriver.
      "tauri:options": { application: bin.app },
    } as WebdriverIO.Capabilities,
  ],
  logLevel: "warn",
  waitforTimeout: 15_000,
  connectionRetryTimeout: 120_000,
  connectionRetryCount: 1,
  framework: "mocha",
  mochaOpts: { ui: "bdd", timeout: 180_000 },
  reporters: ["spec"],

  beforeSession: async () => {
    await prepareFresh();
  },

  afterTest: async (test, _context, { passed }) => {
    if (passed) return;
    mkdirSync(artifactsDir, { recursive: true });
    const name = `${test.parent} ${test.title}`.replace(/[^\w-]+/g, "_").slice(0, 120);
    await browser.saveScreenshot(join(artifactsDir, `${name}.png`));
    writeFileSync(join(artifactsDir, `${name}.html`), await browser.getPageSource());
  },

  afterSession: async () => {
    netsim.stop();
    await stopDriver();
  },
};
