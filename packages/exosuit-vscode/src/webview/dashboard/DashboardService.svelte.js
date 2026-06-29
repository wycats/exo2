import { BridgeReceiver } from "../bridge/Receiver";
export class DashboardService {
    constructor() {
        this.bridge = new BridgeReceiver();
        // Service-Based Root State
        this.state = $state(null);
        this.rfcs = $state([]);
        this.feedback = $state([]);
        this.bridge
            .use("dashboard.state", null)
            .subscribe((v) => {
            console.log("[DashboardService] Received state:", v);
            this.state = v;
        });
        this.bridge.use("dashboard.rfcs", []).subscribe((v) => {
            console.log("[DashboardService] Updated rfcs:", v?.length);
            this.rfcs = v;
        });
        this.bridge.use("dashboard.feedback", []).subscribe((v) => {
            this.feedback = v;
        });
    }
    // Pull-Based Reactivity (Derived State)
    get currentPhase() {
        return this.state?.currentPhase;
    }
    get plan() {
        return this.state?.plan;
    }
    get mode() {
        return this.state?.mode || "loading";
    }
    get isLoaded() {
        return this.state !== null;
    }
}
export const dashboardService = new DashboardService();
