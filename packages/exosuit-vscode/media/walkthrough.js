// @ts-check
(function () {
  // @ts-ignore
  const vscode = acquireVsCodeApi();

  const contentDiv = document.getElementById("content");

  window.addEventListener("message", (event) => {
    const message = event.data;
    switch (message.type) {
      case "update":
        updateContent(message.text);
        break;
    }
  });

  /**
   * @param {string} text
   */
  function updateContent(text) {
    if (!contentDiv) return;
    contentDiv.innerHTML = "";

    const lines = text.split("\n");
    lines.forEach((line, index) => {
      const lineDiv = document.createElement("div");
      lineDiv.className = "line";

      // Check for checkboxes
      if (line.trim().startsWith("- [ ]") || line.trim().startsWith("- [x]")) {
        const checkbox = document.createElement("input");
        checkbox.type = "checkbox";
        checkbox.checked = line.includes("[x]");
        checkbox.addEventListener("change", () => {
          vscode.postMessage({
            type: "toggle",
            line: index,
          });
        });
        lineDiv.appendChild(checkbox);

        const label = document.createElement("span");
        const taskText = line.replace(/- \[[ x]\]/, "").trim();
        label.textContent = taskText;
        lineDiv.appendChild(label);

        // Add Verify Button
        const verifyBtn = document.createElement("button");
        verifyBtn.className = "verify-btn";
        verifyBtn.textContent = "Verify";
        verifyBtn.title = "Ask Exosuit to verify this task";
        verifyBtn.addEventListener("click", () => {
          vscode.postMessage({
            type: "verify",
            text: taskText,
          });
        });
        lineDiv.appendChild(verifyBtn);
      } else {
        // Simple markdown rendering for headers
        if (line.startsWith("# ")) {
          const h1 = document.createElement("h1");
          h1.textContent = line.substring(2);
          lineDiv.appendChild(h1);
        } else if (line.startsWith("## ")) {
          const h2 = document.createElement("h2");
          h2.textContent = line.substring(3);
          lineDiv.appendChild(h2);
        } else if (line.startsWith("### ")) {
          const h3 = document.createElement("h3");
          h3.textContent = line.substring(4);
          lineDiv.appendChild(h3);
        } else {
          lineDiv.textContent = line;
        }
      }

      contentDiv.appendChild(lineDiv);
    });
  }

  // Signal that we are ready to receive content
  vscode.postMessage({ type: "ready" });
})();
