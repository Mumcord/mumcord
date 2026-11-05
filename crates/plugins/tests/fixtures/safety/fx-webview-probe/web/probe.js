(async () => {
  const probe = new URLSearchParams(location.search).get("probe") || "http://127.0.0.1:18999/probe";
  const results = {};
  const attempt = async (name, fn) => {
    try {
      await fn();
      results[name] = "reachable";
    } catch (error) {
      results[name] = "blocked: " + error.name;
    }
  };
  await attempt("fetch", () => fetch(probe, { mode: "no-cors" }));
  await attempt("parent-document", () => window.parent.document.title);
  await attempt("top-location", () => window.top.location.href);
  await attempt("local-storage", () => localStorage.getItem("x"));
  // The Tauri IPC must not exist inside the frame, neither as the injected global nor as the
  // raw platform handlers (WebKitGTK / macOS and WebView2).
  results["tauri-internals"] = typeof window.__TAURI_INTERNALS__;
  results["webkit-ipc-handler"] = String(Boolean(window.webkit?.messageHandlers?.ipc));
  results["webview2-handler"] = String(Boolean(window.chrome?.webview));
  await attempt("webkit-ipc-post", () => window.webkit.messageHandlers.ipc.postMessage("probe"));
  await attempt("webview2-post", () => window.chrome.webview.postMessage("probe"));
  const report = JSON.stringify(results);
  // The plugin echoes every page message back, so the echo proves both bridge directions.
  window.addEventListener("message", (event) => {
    if (event.data === report) document.getElementById("echo").textContent = "echo received";
  });
  document.getElementById("out").textContent = report;
  window.parent.postMessage(report, "*");
})();
