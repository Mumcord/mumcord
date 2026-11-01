import { fireEvent, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setApi, type Settings } from "../../api";
import { FeaturesStep } from "./FeaturesStep";
import { StartupStep } from "./StartupStep";
import { SummaryStep } from "./SummaryStep";
import { renderInWizard, setupWizardTest, type Harness } from "./testUtils";
import { VoiceStep } from "./VoiceStep";

let harness: Harness;
let settings: Settings;

beforeEach(async () => {
  harness = await setupWizardTest();
  settings = await harness.api.settingsGet();
});

afterEach(() => {
  setApi(undefined);
});

describe("VoiceStep", () => {
  it("offers the noise suppression levels of Settings and reports a choice", () => {
    const update = vi.fn();
    renderInWizard(
      harness,
      <VoiceStep
        settings={settings}
        update={update}
        availability={null}
        mouseStatus={null}
        onNext={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("radio", { name: "Off" }));
    expect(update).toHaveBeenCalledWith({ noiseSuppression: false });
    fireEvent.click(screen.getByRole("radio", { name: /Advanced \(AI\)/ }));
    expect(update).toHaveBeenCalledWith({
      noiseSuppression: true,
      noiseSuppressionBackend: "advanced",
    });
  });
});

describe("FeaturesStep", () => {
  function renderFeatures(ingameOverlay: boolean | null) {
    const changes = { overlay: vi.fn(), dnd: vi.fn(), update: vi.fn() };
    renderInWizard(
      harness,
      <FeaturesStep
        settings={settings}
        update={changes.update}
        ingameOverlay={ingameOverlay}
        dndFollowSystem
        onIngameOverlayChange={changes.overlay}
        onDndFollowSystemChange={changes.dnd}
        onNext={vi.fn()}
      />,
    );
    return changes;
  }

  it("reports each switch and explains it behind a question mark", () => {
    const changes = renderFeatures(false);
    fireEvent.click(screen.getByRole("switch", { name: "Share what I'm listening to" }));
    expect(changes.update).toHaveBeenCalledWith({ nowPlaying: !settings.nowPlaying });
    fireEvent.click(screen.getByRole("switch", { name: "In-game overlay" }));
    expect(changes.overlay).toHaveBeenCalledWith(true);
    fireEvent.click(screen.getByRole("switch", { name: /Follow system Do Not Disturb/ }));
    expect(changes.dnd).toHaveBeenCalledWith(false);
    expect(screen.getByRole("button", { name: "About In-game overlay" })).toBeInTheDocument();
  });

  it("hides the in-game overlay where it is not supported", () => {
    renderFeatures(null);
    expect(screen.queryByRole("switch", { name: "In-game overlay" })).not.toBeInTheDocument();
  });
});

describe("StartupStep", () => {
  it("allows start minimized only with start at login, and knows an active link handler", () => {
    renderInWizard(
      harness,
      <StartupStep
        settings={{ ...settings, autostart: false }}
        update={vi.fn()}
        linkHandlerActive
        registerLinkHandler={false}
        onRegisterLinkHandlerChange={vi.fn()}
        onNext={vi.fn()}
      />,
    );
    expect(screen.getByRole("switch", { name: "Start minimized" })).toBeDisabled();
    expect(screen.getByText("Mumcord already opens mumble:// links.")).toBeInTheDocument();
    expect(screen.getByRole("radiogroup", { name: "When an update is found" })).toBeInTheDocument();
  });
});

describe("SummaryStep", () => {
  it("lists every choice with Edit and reminds of the identity backup", async () => {
    const onEdit = vi.fn();
    const identity = await harness.api.identityCreate("Mira", null);
    renderInWizard(
      harness,
      <SummaryStep
        draft={{ settings, ingameOverlay: true, dndFollowSystem: true, registerLinkHandler: true }}
        identity={identity}
        name="Mira"
        firstStart
        busy={false}
        error={null}
        onEdit={onEdit}
        onFinish={vi.fn()}
      />,
    );
    expect(screen.getByRole("heading", { name: "You're all set, Mira" })).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: /^Edit / })).toHaveLength(9);
    fireEvent.click(screen.getByRole("button", { name: "Edit Chat content" }));
    expect(onEdit).toHaveBeenCalledWith("connections");
    expect(screen.getByText(/Your identity Mira is saved/)).toBeInTheDocument();
    expect(screen.getByText("Next: add your first server")).toBeInTheDocument();
  });
});
