// Spec 08, camera availability (AC-8.1, AC-8.2) in the running client: the user-panel camera
// button and the Apps-menu entry are disabled with the reason, or enabled, depending on the server's
// relay (mumpeer testenv profiles, helpers/relayenv.ts) and the own peer-to-peer mode.
//
//   AC-8.1  a server without a relay (the plain v15 test server) → "This server has no relay, so
//           camera sharing isn't available." on both entry points.
//   AC-8.2  profile `all` → enabled, server info "Relay allows: … · Cameras"; P2P mode "Direct
//           only" → "Camera sharing needs the relay. …"; profile `no-camera` → "This server's
//           relay doesn't allow camera sharing"; profile `old` → "This server's relay is too old
//           for camera sharing" (only with MUMCORD_E2E_OLD_MUMPEER_IMAGE, like AC-7.5).
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

  it("AC-8.1: a server without a relay disables the camera with the reason", async () => {
    await testenv.up("v15");
    await app.restart({ freshProfile: true, env: camera.CAMERA_ENV });
    await completeWizard("Alice");
    await addAndConnect();
    await camera.joinVoice();
    await camera.expectCameraBlock(camera.REASON.noRelay);
  });

  it("AC-8.2: a relay that allows cameras enables both entry points and says so in server info", async () => {
    const relay = await relayenv.startRelay("all");
    await camera.startA("Alice", relay.port);
    expect((await relayenv.relayStatus()).uses).toContain("camera");
    await camera.expectCameraBlock(null);
    expect(await camera.relayAllows()).toMatch(/· Cameras$/);
  });

  it("AC-8.2: P2P mode Direct only asks for the relay", async () => {
    await camera.setP2pMode("directOnly");
    await camera.expectCameraBlock(camera.REASON.needsRelayMode);
    await camera.setP2pMode("directRelay");
    await camera.expectCameraBlock(null);
  });

  it("AC-8.2: profile no-camera says the relay doesn't allow cameras", async () => {
    // The same worker keeps its registration across the switch; the client reconnects on its own.
    await relayenv.startRelay("no-camera");
    expect((await relayenv.relayStatus()).uses).not.toContain("camera");
    await camera.expectCameraBlock(camera.REASON.relayDisallows, 90_000);
    expect(await camera.relayAllows()).not.toContain("Cameras");
  });

  (oldImage ? it : it.skip)("AC-8.2: profile old says the relay is too old", async () => {
    await relayenv.startRelay("old", { image: oldImage, botImage: oldBotImage });
    await camera.expectCameraBlock(camera.REASON.relayTooOld, 90_000);
  });
});
