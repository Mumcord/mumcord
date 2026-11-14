// Spec 08, camera availability (AC-8.1, AC-8.2) in the running client: the user-panel camera
// button and the Apps-menu entry are disabled with the reason, or enabled, depending on the server's
// relay (mumpeer testenv profiles, helpers/relayenv.ts) and the own peer-to-peer mode.
//
//   AC-8.1  a server without a relay (the plain v15 test server), default P2P mode → the camera
//           stays available (two 0.9.0 clients use the direct path, FR-8.96). The same server in
//           `relay_only` → "This server has no relay, so camera sharing isn't available."
//   AC-8.2  profile `all` → enabled, server info "Relay allows: … · Cameras". `direct_only` stays
//           enabled. `off` → "Camera sharing is turned off. …". `direct_only` with direct
//           connections refused → "Camera sharing needs direct connections. …". Profile
//           `no-camera`: the default mode stays available over the direct path; `relay_only` →
//           "This server's relay doesn't allow camera sharing." Profile `old` in `relay_only` →
//           "This server's relay is too old for camera sharing" (only with
//           MUMCORD_E2E_OLD_MUMPEER_IMAGE, like AC-7.5).
//
// Skips itself without a mumpeer checkout.
import { expect } from "@wdio/globals";
import { app, relayenv, testenv } from "../../helpers/index.ts";
import * as camera from "../../helpers/camera.ts";
import { addAndConnect, completeWizard } from "../../helpers/voice-ui.ts";

const oldImage = process.env.MUMCORD_E2E_OLD_MUMPEER_IMAGE;
const oldBotImage = process.env.MUMCORD_E2E_OLD_MUMBOT_IMAGE;

(relayenv.relayAvailable() ? describe : describe.skip)("camera availability (spec 08, AC-8.1, 8.2)", function () {
  this.timeout(900_000);
  this.bail(true);

  it("AC-8.1: without a relay the camera stays available, and relay-only names the reason", async () => {
    await testenv.up("v15");
    await app.restart({ freshProfile: true, env: camera.CAMERA_ENV });
    await completeWizard("Alice");
    await addAndConnect();
    await camera.joinVoice();
    // Default mode (relay first): the direct path is left, so the entries stay enabled (FR-8.96).
    await camera.expectCameraBlock(null);
    await camera.setP2pMode("relayOnly");
    await camera.expectCameraBlock(camera.REASON.noRelay);
  });

  it("AC-8.2: a relay that allows cameras enables both entry points and says so in server info", async () => {
    const relay = await relayenv.startRelay("all");
    await camera.startA("Alice", relay.port);
    expect((await relayenv.relayStatus()).uses).toContain("camera");
    await camera.expectCameraBlock(null);
    expect(await camera.relayAllows()).toMatch(/· Cameras$/);
  });

  it("AC-8.2: Direct only stays available, Off turns the camera off, and refusing direct connections asks for them", async () => {
    await camera.setP2pMode("directOnly");
    await camera.expectCameraBlock(null);
    await camera.setP2pMode("off");
    await camera.expectCameraBlock(camera.REASON.needsRelayMode);
    await camera.setP2pMode("directOnly");
    const settings = await app.invoke<Record<string, unknown>>("settings_get");
    await app.invoke("settings_update", {
      settings: { ...settings, allowDirectConnections: "no" },
    });
    await camera.expectCameraBlock(camera.REASON.needsDirect);
    // The next cells need the direct path again, and the default mode.
    await app.invoke("settings_update", {
      settings: { ...settings, allowDirectConnections: "yes" },
    });
    await camera.setP2pMode("relayFirst");
    await camera.expectCameraBlock(null);
  });

  it("AC-8.2: profile no-camera keeps the direct path, and relay-only names the relay", async () => {
    // The same worker keeps its registration across the switch; the client reconnects on its own.
    await relayenv.startRelay("no-camera");
    expect((await relayenv.relayStatus()).uses).not.toContain("camera");
    await camera.setP2pMode("relayFirst");
    await camera.expectCameraBlock(null);
    await camera.setP2pMode("relayOnly");
    await camera.expectCameraBlock(camera.REASON.relayDisallows, 90_000);
    expect(await camera.relayAllows()).not.toContain("Cameras");
  });

  (oldImage ? it : it.skip)("AC-8.2: profile old in relay-only says the relay is too old", async () => {
    await relayenv.startRelay("old", { image: oldImage, botImage: oldBotImage });
    await camera.setP2pMode("relayOnly");
    await camera.expectCameraBlock(camera.REASON.relayTooOld, 90_000);
  });
});
