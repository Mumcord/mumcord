import { describe, expect, it } from "vitest";
import {
  AV1_TYPE,
  H264_TYPE,
  installHint,
  installLine,
  isCodecFailure,
  isTwitchPlayer,
  missingPackages,
  probeCodecs,
  supportsH264,
  type MediaCodecs,
} from "./codecSupport";

describe("isCodecFailure", () => {
  it("counts YouTube's HTML5 error and any Twitch player", () => {
    expect(isCodecFailure({ kind: "youtube", code: 5 })).toBe(true);
    expect(isCodecFailure({ kind: "twitch" })).toBe(true);
    // YouTube paints its own panel for a missing decoder and sends no onError (FR-13.35).
    expect(isCodecFailure({ kind: "stalled" })).toBe(true);
  });

  it("ignores other YouTube errors", () => {
    for (const code of [0, 2, 100, 101, 150, 153]) {
      expect(isCodecFailure({ kind: "youtube", code })).toBe(false);
    }
  });
});

describe("isTwitchPlayer", () => {
  it("recognises Twitch's player and clip origins only", () => {
    expect(isTwitchPlayer("https://player.twitch.tv/?channel=abc")).toBe(true);
    expect(isTwitchPlayer("https://clips.twitch.tv/embed?clip=x")).toBe(true);
    expect(isTwitchPlayer("https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ")).toBe(false);
    expect(isTwitchPlayer("not a url")).toBe(false);
  });
});

describe("supportsH264", () => {
  it("asks for H.264 in MP4 and takes either answer", () => {
    const asked: string[] = [];
    const probe = (mse: boolean | null, element: string) => ({
      mse:
        mse === null
          ? null
          : (type: string) => {
              asked.push(type);
              return mse;
            },
      element: () => element,
    });
    expect(supportsH264(probe(true, ""))).toBe(true);
    expect(supportsH264(probe(false, "maybe"))).toBe(true);
    expect(supportsH264(probe(null, "probably"))).toBe(true);
    expect(supportsH264(probe(false, ""))).toBe(false);
    expect(supportsH264(probe(null, ""))).toBe(false);
    expect(asked.every((type) => type === H264_TYPE)).toBe(true);
  });
});

describe("installHint", () => {
  const distro = (id: string, idLike: string[] = []) => ({ id, idLike });

  it("picks the package manager from the distribution ID", () => {
    expect(installHint(distro("ubuntu")).command).toBe("sudo apt install gstreamer1.0-libav");
    expect(installHint(distro("debian")).command).toBe("sudo apt install gstreamer1.0-libav");
    expect(installHint(distro("fedora"))).toEqual({
      command: "sudo dnf install gstreamer1-libav",
      rpmFusion: true,
    });
    expect(installHint(distro("arch")).command).toBe("sudo pacman -S gst-libav");
    expect(installHint(distro("opensuse-tumbleweed")).command).toBe(
      "sudo zypper install gstreamer-plugins-libav",
    );
  });

  it("falls back to ID_LIKE for derivatives", () => {
    expect(installHint(distro("linuxmint", ["ubuntu", "debian"])).command).toBe(
      "sudo apt install gstreamer1.0-libav",
    );
    expect(installHint(distro("manjaro", ["arch"])).command).toBe("sudo pacman -S gst-libav");
    expect(installHint(distro("rocky", ["rhel", "centos", "fedora"])).rpmFusion).toBe(true);
  });

  it("only the Fedora family gets the RPM Fusion note", () => {
    expect(installHint(distro("ubuntu")).rpmFusion).toBe(false);
    expect(installHint(distro("opensuse-leap", ["suse", "opensuse"])).rpmFusion).toBe(false);
  });

  it("is generic for unknown or missing distributions", () => {
    expect(installHint(distro("nixos"))).toEqual({ command: null, rpmFusion: false });
    expect(installHint(distro("gentoo"))).toEqual({ command: null, rpmFusion: false });
    expect(installHint(null)).toEqual({ command: null, rpmFusion: false });
  });
});

describe("probeCodecs", () => {
  it("asks for every decoder the app cares about", () => {
    const asked: string[] = [];
    const codecs = probeCodecs({
      mse: (type) => {
        asked.push(type);
        return type === 'video/mp4; codecs="avc1.42E01E"';
      },
      element: () => "",
    });
    expect(codecs).toEqual({ h264: true, vp9: false, opus: false, av1: false });
    expect(asked).toEqual([
      H264_TYPE,
      'video/webm; codecs="vp9"',
      'audio/webm; codecs="opus"',
      AV1_TYPE,
    ]);
  });
});

describe("missingPackages", () => {
  const all: MediaCodecs = { h264: true, vp9: true, opus: true, av1: false };

  it("names the packages in the order a YouTube video needs them", () => {
    expect(missingPackages(all)).toEqual([]);
    expect(missingPackages({ ...all, h264: false }).map((e) => e.package)).toEqual([
      "gstreamer1-libav",
    ]);
    expect(missingPackages({ ...all, vp9: false, opus: false }).map((e) => e.package)).toEqual([
      "gstreamer1-plugins-vpx",
      "gstreamer1-plugins-opus",
    ]);
  });

  it("never offers a package for AV1, which the collection does not have", () => {
    expect(missingPackages({ ...all, av1: false })).toEqual([]);
  });
});

describe("installLine", () => {
  it("builds the pkg command, and nothing when all decoders are there", () => {
    expect(installLine(missingPackages({ h264: false, vp9: false, opus: true, av1: false }))).toBe(
      "sudo pkg install gstreamer1-libav gstreamer1-plugins-vpx",
    );
    expect(installLine([])).toBeNull();
  });
});
