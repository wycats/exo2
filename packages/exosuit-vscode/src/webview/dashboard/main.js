import { mount } from "svelte";
import App from "./App.svelte";
import "./DashboardService.svelte"; // Ensure service is initialized
import { setVsCodeApi } from "../services/ConsistencyService.svelte";
import { vscode } from "./vscode";
try {
    console.log("[Dashboard Webview] Starting...");
    setVsCodeApi(vscode);
    const root = document.getElementById("app");
    const app = mount(App, {
        target: root,
    });
    // Signal that the webview is ready to receive data
    console.log("[Dashboard Webview] Sending WEBVIEW_READY");
    vscode.postMessage({ type: "WEBVIEW_READY" });
}
catch (e) {
    console.error("[Dashboard Webview] Critical Error:", e);
    vscode.postMessage({
        type: "ERROR",
        payload: { message: e.toString() },
    });
}
// export default app; // Cannot export inside try/catch easily, but we don't need to export it really
