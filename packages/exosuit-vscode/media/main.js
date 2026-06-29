(function () {
  let vscode;
  try {
    vscode = acquireVsCodeApi();
  } catch (e) {}

  function log(message) {
    if (!vscode) {
      return;
    }
    vscode.postMessage({
      type: "log",
      level: "trace",
      component: "webview",
      message,
    });
  }

  log("Webview script initialized");

  function toggleDiff(checked) {
    log("Toggle diff clicked: " + checked);
    if (vscode) {
      vscode.postMessage({ type: "toggleDiff", value: checked });
    } else {
      log("Error: VS Code API not available");
    }
  }

  const diffToggle = document.getElementById("diffToggle");
  if (diffToggle) {
    log("Found diffToggle element");
    diffToggle.addEventListener("change", (e) => {
      toggleDiff(e.target.checked);
    });
  } else {
    log("diffToggle element not found (controls might be hidden)");
  }

  window.addEventListener("message", (event) => {
    const message = event.data;
    if (message.type === "updateLogs") {
      const logsEl = document.getElementById("logs");
      if (logsEl) {
        logsEl.innerHTML = message.logs;
        window.scrollTo(0, document.body.scrollHeight);
      }
    }
  });
})();
