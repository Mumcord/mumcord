// Isolation probes of Test Surface (spec 09: S0, AC-9.2, AC-9.3, AC-9.4, AC-9.24).
//
// Every probe tries one way out of the sandboxed frame and reports a status:
//   blocked  the attempt failed inside the page
//   leaked   the attempt visibly succeeded (a failure of the platform)
//   ok       an allowed action worked (declared fetch, WebAssembly under the CSP)
//   sent     the page cannot tell; the HTTPS fixture's request log decides
// Undeclared requests go to /probe/<name> so the fixture can attribute each hit.
// Results show in #results (data-status) and go to the plugin as result:<place>:<probe>:<status>.
"use strict";

const PLACE = document.currentScript.dataset.place;
const DECLARED = "https://declared.plugins.test:47443";
const UNDECLARED = "https://undeclared.plugins.test:47443";
const STUN = "stun:undeclared.plugins.test:47478";
const WAIT_MS = 3000;

const send = (message) => window.parent.postMessage(message, "*");
const target = (name) => `${UNDECLARED}/probe/${name}`;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// Resolves with `onSuccess` or `onFailure` depending on which event fires first; `blocked` if
// neither fires in time.
function race(element, success, failure, start) {
  return new Promise((resolve) => {
    const timer = setTimeout(() => resolve("blocked"), WAIT_MS);
    const done = (status) => {
      clearTimeout(timer);
      resolve(status);
    };
    element.addEventListener(success, () => done("leaked"), { once: true });
    element.addEventListener(failure, () => done("blocked"), { once: true });
    start();
  });
}

function addLink(rel, name, extra) {
  const link = document.createElement("link");
  link.rel = rel;
  Object.assign(link, extra);
  return race(link, "load", "error", () => {
    link.href = target(name);
    document.head.append(link);
  });
}

async function fetchStatus(url) {
  try {
    await fetch(url, { cache: "no-store" });
    return "leaked";
  } catch {
    return "blocked";
  }
}

// Probes that would navigate the frame away. They are not part of "run all"; a navigation that
// works removes the page, so the missing result plus the fixture's log show the leak.
async function stillHere(start) {
  try {
    start();
  } catch {
    return "blocked";
  }
  await sleep(WAIT_MS);
  return "blocked";
}

const PROBES = {
  "fetch-declared": async () => {
    try {
      const response = await fetch(`${DECLARED}/`, { cache: "no-store" });
      return response.ok ? "ok" : "blocked";
    } catch {
      return "blocked";
    }
  },
  "fetch-undeclared": () => fetchStatus(target("fetch")),
  "fetch-http": () => fetchStatus("http://declared.plugins.test:47443/probe/http"),
  xhr: () =>
    new Promise((resolve) => {
      const xhr = new XMLHttpRequest();
      xhr.onload = () => resolve("leaked");
      xhr.onerror = () => resolve("blocked");
      try {
        xhr.open("GET", target("xhr"));
        xhr.send();
      } catch {
        resolve("blocked");
      }
    }),
  webrtc: async () => {
    if (typeof RTCPeerConnection !== "function") return "blocked";
    let connection;
    try {
      connection = new RTCPeerConnection({ iceServers: [{ urls: STUN }] });
    } catch {
      return "blocked";
    }
    // A server-reflexive candidate means the STUN request left the machine's sandbox.
    let reflexive = false;
    connection.addEventListener("icecandidate", (event) => {
      if (event.candidate && / typ (srflx|relay)/.test(event.candidate.candidate)) reflexive = true;
    });
    try {
      connection.createDataChannel("probe");
      await connection.setLocalDescription(await connection.createOffer());
    } catch {
      connection.close();
      return "blocked";
    }
    await sleep(WAIT_MS);
    connection.close();
    return reflexive ? "leaked" : "blocked";
  },
  "img-beacon": () => {
    const image = new Image();
    return race(image, "load", "error", () => (image.src = target("img")));
  },
  "send-beacon": () => {
    try {
      return Promise.resolve(navigator.sendBeacon(target("beacon"), "x") ? "sent" : "blocked");
    } catch {
      return Promise.resolve("blocked");
    }
  },
  prefetch: () => addLink("prefetch", "prefetch").then((s) => (s === "blocked" ? "sent" : s)),
  preconnect: () => addLink("preconnect", "preconnect").then(() => "sent"),
  "dns-prefetch": () => addLink("dns-prefetch", "dns-prefetch").then(() => "sent"),
  preload: () => addLink("preload", "preload", { as: "fetch", crossOrigin: "anonymous" }),
  "css-import": () => addLink("stylesheet", "css"),
  "css-url": async () => {
    document.getElementById("css-url").style.backgroundImage = `url("${target("bg")}")`;
    await sleep(500);
    return "sent";
  },
  eventsource: () => {
    try {
      const source = new EventSource(target("events"));
      return race(source, "open", "error", () => {}).finally(() => source.close());
    } catch {
      return Promise.resolve("blocked");
    }
  },
  websocket: () => {
    try {
      const socket = new WebSocket("wss://undeclared.plugins.test:47443/ws");
      return race(socket, "open", "error", () => {}).finally(() => socket.close());
    } catch {
      return Promise.resolve("blocked");
    }
  },
  "a-ping": async () => {
    const link = document.createElement("a");
    link.href = target("ping-page");
    link.target = "_blank";
    link.setAttribute("ping", target("ping"));
    document.body.append(link);
    link.click();
    link.remove();
    await sleep(500);
    return "sent";
  },
  download: async () => {
    const link = document.createElement("a");
    link.href = target("download");
    link.download = "probe.txt";
    document.body.append(link);
    link.click();
    link.remove();
    await sleep(500);
    return "sent";
  },
  "window-open": () => {
    try {
      const popup = window.open(target("popup"), "_blank");
      if (popup) {
        popup.close();
        return Promise.resolve("leaked");
      }
      return Promise.resolve("blocked");
    } catch {
      return Promise.resolve("blocked");
    }
  },
  workers: () => {
    try {
      const code = `fetch(${JSON.stringify(target("worker"))}).then(() => postMessage("leaked"), () => postMessage("blocked"));`;
      const worker = new Worker(URL.createObjectURL(new Blob([code], { type: "text/javascript" })));
      return new Promise((resolve) => {
        const timer = setTimeout(() => resolve("blocked"), WAIT_MS);
        worker.onmessage = (event) => {
          clearTimeout(timer);
          resolve(event.data === "leaked" ? "leaked" : "blocked");
        };
        worker.onerror = () => {
          clearTimeout(timer);
          resolve("blocked");
        };
      }).finally(() => worker.terminate());
    } catch {
      return Promise.resolve("blocked");
    }
  },
  "remote-script": () => {
    const script = document.createElement("script");
    return race(script, "load", "error", () => {
      script.src = target("remote.js");
      document.head.append(script);
    });
  },
  "csp-eval": () => {
    try {
      eval("1");
      return Promise.resolve("leaked");
    } catch {
      return Promise.resolve("blocked");
    }
  },
  // The CSP allows 'wasm-unsafe-eval', so a module must compile.
  "csp-wasm": () =>
    WebAssembly.compile(new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0])).then(
      () => "ok",
      () => "blocked",
    ),
  modals: () => {
    // Without allow-modals these return at once instead of showing a dialog.
    const started = Date.now();
    try {
      window.alert("probe");
      const confirmed = window.confirm("probe");
      const answer = window.prompt("probe");
      const shown = confirmed || answer !== null || Date.now() - started > 1000;
      return Promise.resolve(shown ? "leaked" : "blocked");
    } catch {
      return Promise.resolve("blocked");
    }
  },
  ipc: () => {
    const names = ["__TAURI__", "__TAURI_INTERNALS__", "__TAURI_IPC__", "ipc"];
    const reachable = (scope) => {
      try {
        return names.some((name) => scope[name] !== undefined);
      } catch {
        return false;
      }
    };
    return Promise.resolve(
      reachable(window) || reachable(window.parent) || reachable(window.top) ? "leaked" : "blocked",
    );
  },
  "parent-dom": () => {
    try {
      return Promise.resolve(window.parent.document.body ? "leaked" : "blocked");
    } catch {
      return Promise.resolve("blocked");
    }
  },
  // Other frames in the app window (this plugin's other places, other plugins): reading their
  // document must fail like the parent's.
  "other-frames": () => {
    let reached = false;
    try {
      const frames = window.parent.frames;
      for (let i = 0; i < frames.length; i++) {
        if (frames[i] === window) continue;
        try {
          if (frames[i].document.body) reached = true;
        } catch {
          // a cross-origin frame: blocked, as expected
        }
      }
    } catch {
      return Promise.resolve("blocked");
    }
    return Promise.resolve(reached ? "leaked" : "blocked");
  },
  storage: () => {
    try {
      window.localStorage.setItem("probe", "1");
      return Promise.resolve("leaked");
    } catch {
      // fall through to the next store
    }
    try {
      window.sessionStorage.setItem("probe", "1");
      return Promise.resolve("leaked");
    } catch {
      return Promise.resolve("blocked");
    }
  },
  cookies: () => {
    try {
      document.cookie = "probe=1";
      return Promise.resolve(document.cookie.includes("probe=1") ? "leaked" : "blocked");
    } catch {
      return Promise.resolve("blocked");
    }
  },
  clipboard: async () => {
    try {
      await navigator.clipboard.readText();
      return "leaked";
    } catch {
      return "blocked";
    }
  },
  media: async () => {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true, video: true });
      stream.getTracks().forEach((track) => track.stop());
      return "leaked";
    } catch {
      return "blocked";
    }
  },
  indexeddb: async () => {
    try {
      const request = window.indexedDB.open("probe");
      return await race(request, "success", "error", () => {});
    } catch {
      return "blocked";
    }
  },
  navigation: () => stillHere(() => window.location.assign(target("nav"))),
  "top-navigation": () => stillHere(() => window.top.location.assign(target("top-nav"))),
  "meta-refresh": () =>
    stillHere(() => {
      const meta = document.createElement("meta");
      meta.httpEquiv = "refresh";
      meta.content = `0;url=${target("refresh")}`;
      document.head.append(meta);
    }),
  form: () =>
    stillHere(() => {
      const form = document.createElement("form");
      form.method = "post";
      form.action = target("form");
      document.body.append(form);
      form.submit();
    }),
};

// A download link to another origin is one too: WebKitGTK ignores `download` across origins and
// follows the link, and the host closes a frame that loads a second page.
const NAVIGATING = new Set(["navigation", "top-navigation", "meta-refresh", "form", "download"]);

function resultItem(probe) {
  const item = document.createElement("li");
  item.id = `result-${probe}`;
  item.dataset.status = "idle";
  item.append(`${probe}: `, document.createElement("output"));
  document.getElementById("results").append(item);
  return item;
}

function show(probe, status) {
  const item = document.getElementById(`result-${probe}`) ?? resultItem(probe);
  item.dataset.status = status;
  item.querySelector("output").textContent = status;
  send(`result:${PLACE}:${probe}:${status}`);
}

async function run(probe) {
  show(probe, "running");
  let status;
  try {
    status = await PROBES[probe]();
  } catch {
    status = "error";
  }
  show(probe, status);
}

async function runAll() {
  for (const probe of Object.keys(PROBES)) {
    if (!NAVIGATING.has(probe)) await run(probe);
  }
}

function busy() {
  // Never returns: the heartbeat stops and the host's watchdog must remove the frame (AC-9.4).
  for (;;) {
    // spin
  }
}

function hog() {
  const chunks = [];
  for (;;) chunks.push(new Float64Array(1 << 20).fill(1));
}

// Frame audio is allowed only after a click inside the frame (AC-9.24): the button plays a tone,
// the plugin's "tone" command tries the same without a click and reports the context state.
// The context is shared and resumed on every call, so a context that a call without a click
// created (suspended) still sounds after a later click, as a real page would do it.
let audio;
async function tone() {
  audio ??= new AudioContext();
  const oscillator = audio.createOscillator();
  oscillator.frequency.value = 440;
  oscillator.connect(audio.destination);
  oscillator.start();
  oscillator.stop(audio.currentTime + 2);
  try {
    await audio.resume();
  } catch {
    // Refused without a click inside the frame: the state below says "blocked".
  }
  return audio.state === "running" ? "ok" : "blocked";
}

function build() {
  const buttons = document.getElementById("probes");
  for (const probe of Object.keys(PROBES)) {
    const button = document.createElement("button");
    button.type = "button";
    button.id = `probe-${probe}`;
    button.textContent = probe;
    button.addEventListener("click", () => run(probe));
    buttons.append(button);
    resultItem(probe);
  }
  document.getElementById("run-all").addEventListener("click", runAll);
  document.getElementById("busy").addEventListener("click", busy);
  // A gesture-bound host call asked for from inside the page (FR-9.45): it works only after a
  // trusted click here, never after a script-made one.
  document
    .getElementById("start-stage")
    ?.addEventListener("click", () => send(`start-stage:${PLACE}`));
  document.getElementById("tone").addEventListener("click", () => {
    void tone().then((status) => show("tone-click", status));
  });
}

window.addEventListener("message", (event) => {
  if (event.source !== window.parent || typeof event.data !== "string") return;
  const [command, place, probe] = event.data.split(":");
  if (place !== "*" && place !== PLACE) return;
  if (command === "run" && probe === "all") runAll();
  else if (command === "run" && Object.hasOwn(PROBES, probe)) run(probe);
  else if (command === "busy") busy();
  else if (command === "hog") hog();
  else if (command === "tone") void tone().then((status) => show("tone-auto", status));
});

build();
document.getElementById("place").textContent = PLACE;
send(`ready:${PLACE}`);
