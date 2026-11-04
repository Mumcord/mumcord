// Injected by Mumcord in front of every plugin page and allowed by the page's CSP through its
// sha256 hash, which Mumcord computes from this file, so edits need no other change. It runs before the
// page's own code. Messages to the app are objects with a `mumcordWebview` key, never strings,
// so the app tells them apart from the plugin's bridge messages; a page faking one only hurts
// itself (it loses focus, looks frozen or shows a mute button). Gesture reports are the
// exception: they carry a key the page never sees, see below.
(function () {
  "use strict";
  var host = parent;
  // Taken before the page's code runs, which may replace prototype methods and getters later.
  var uncurry = Function.prototype.bind.bind(Function.prototype.call);
  var messageSource = uncurry(Object.getOwnPropertyDescriptor(MessageEvent.prototype, "source").get);
  var messageData = uncurry(Object.getOwnPropertyDescriptor(MessageEvent.prototype, "data").get);
  var stopEvent = uncurry(Event.prototype.stopImmediatePropagation);
  var dateNow = Date.now;

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
  // FR-9.45: a genuine click or key press inside the page is a user gesture for the plugin.
  // Right after the page loaded, the app hands this frame a random key in a message that only
  // this listener sees (it runs first and stops the event). Reports carry the key, so a page
  // posting the same shape itself is ignored; script-made events are never reported.
  var GESTURE_REPORT_MS = 250;
  var key = null;
  var lastReport = -Infinity;
  addEventListener(
    "message",
    function (event) {
      var data = messageData(event);
      if (messageSource(event) !== host || typeof data !== "object" || data === null) return;
      if (data.mumcordWebview !== "key") return;
      stopEvent(event);
      if (key === null && typeof data.key === "string") key = data.key;
    },
    true,
  );
  function reportGesture(event) {
    if (!event.isTrusted || key === null) return;
    var now = dateNow();
    if (now - lastReport < GESTURE_REPORT_MS) return;
    lastReport = now;
    host.postMessage({ mumcordWebview: "gesture", key: key }, "*");
  }
  ["pointerdown", "click", "keydown"].forEach(function (type) {
    addEventListener(type, reportGesture, true);
  });

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
