// Injected by Mumcord in front of every plugin page and allowed by the page's CSP through its
// sha256 hash, which Mumcord computes from this file, so edits need no other change. It runs before the
// page's own code. Messages to the app are objects with a `mumcordWebview` key, never strings,
// so the app tells them apart from the plugin's bridge messages; a page faking one only hurts
// itself (it loses focus, looks frozen or shows a mute button).
(function () {
  "use strict";
  var host = parent;

  // Backup for the WebRTC browser flag on Windows (a WebRTC peer connection can leak the user's
  // IP addresses past the frame proxy): the page never sees the WebRTC constructors.
  ["RTCPeerConnection", "webkitRTCPeerConnection", "RTCDataChannel"].forEach(function (name) {
    try {
      delete window[name];
    } catch (e) {
      // A non-configurable property cannot be removed; the browser flag still applies.
    }
  });

  function post(kind) {
    host.postMessage({ mumcordWebview: kind }, "*");
  }

  // FR-9.42: the app removes the frame when this stops for 3 s (a busy page loop).
  setInterval(function () {
    post("heartbeat");
  }, 1000);

  // FR-9.41: key events inside the frame never reach the app, so the page tells it. Escape also
  // closes a dialog; F6 only returns focus.
  addEventListener(
    "keydown",
    function (event) {
      if (event.key === "Escape") post("escape");
      else if (event.key === "F6") post("f6");
    },
    true,
  );

  // FR-9.43: sound only after a click inside the frame, muted on the app's request (mute button,
  // deafened). The originals live only in this closure; the page runs after it.
  var clicked = false;
  var announced = false;
  var hostMuted = false;
  var media = HTMLMediaElement.prototype;
  var play = media.play;
  var mutedProperty = Object.getOwnPropertyDescriptor(media, "muted");
  var pageMuted = new WeakMap();
  var playing = new Set();
  // Every audio context with its original resume and suspend.
  var contexts = new Map();

  function refused() {
    return Promise.reject(
      new DOMException("Sound starts after a click inside the plugin.", "NotAllowedError"),
    );
  }
  function sounding() {
    if (!announced) {
      announced = true;
      post("sound");
    }
  }
  function applyMute(element) {
    mutedProperty.set.call(element, hostMuted || pageMuted.get(element) === true);
  }
  function track(element) {
    playing.add(element);
    applyMute(element);
    sounding();
  }

  Object.defineProperty(media, "muted", {
    configurable: false,
    enumerable: true,
    get: function () {
      return pageMuted.has(this) ? pageMuted.get(this) : mutedProperty.get.call(this);
    },
    set: function (value) {
      pageMuted.set(this, Boolean(value));
      applyMute(this);
    },
  });
  media.play = function () {
    if (!clicked) return refused();
    track(this);
    return play.apply(this, arguments);
  };
  // Covers `autoplay` and anything that starts playback without calling play().
  addEventListener(
    "play",
    function (event) {
      var element = event.target;
      if (!(element instanceof HTMLMediaElement)) return;
      if (clicked) track(element);
      else element.pause();
    },
    true,
  );

  [window.AudioContext, window.webkitAudioContext].forEach(function (Original) {
    if (typeof Original !== "function") return;
    var proto = Original.prototype;
    var resume = proto.resume;
    var suspend = proto.suspend;
    var Wrapped = function (options) {
      var context = new Original(options);
      contexts.set(context, { resume: resume, suspend: suspend });
      if (!clicked || hostMuted) suspend.call(context);
      else sounding();
      return context;
    };
    Wrapped.prototype = proto;
    proto.constructor = Wrapped;
    proto.resume = function () {
      if (!clicked) return refused();
      if (hostMuted) return Promise.resolve();
      sounding();
      return resume.call(this);
    };
    if (window.AudioContext === Original) window.AudioContext = Wrapped;
    else window.webkitAudioContext = Wrapped;
  });

  addEventListener(
    "pointerdown",
    function (event) {
      if (event.isTrusted) clicked = true;
    },
    true,
  );
  addEventListener("message", function (event) {
    var data = event.data;
    if (event.source !== host || typeof data !== "object" || data === null) return;
    if (data.mumcordWebview !== "mute") return;
    hostMuted = data.muted === true;
    playing.forEach(applyMute);
    contexts.forEach(function (original, context) {
      if (hostMuted) original.suspend.call(context);
      else if (clicked) original.resume.call(context);
    });
  });
})();
