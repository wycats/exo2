import { BridgeReceiver } from "../bridge/Receiver";
import { getWebviewLogger } from "../lib/logger";

const logger = getWebviewLogger("webview");

type DashboardState = {
  mode: "loading" | "welcome" | "dashboard";
  currentPhase: { title: string; phaseId: string } | null;
  plan: any;
};

export class DashboardService {
  private bridge = new BridgeReceiver();

  // Service-Based Root State
  state = $state<DashboardState | null>(null);
  rfcs = $state<any[]>([]);
  feedback = $state<any[]>([]);

  constructor() {
    this.bridge
      .use<DashboardState | null>("dashboard.state", null)
      .subscribe((v) => {
        logger.debug("[DashboardService] Received state", v);
        this.state = v;
      });
    this.bridge.use<any[]>("dashboard.rfcs", []).subscribe((v) => {
      logger.debug("[DashboardService] Updated rfcs", v?.length);
      this.rfcs = v;
    });
    this.bridge.use<any[]>("dashboard.feedback", []).subscribe((v) => {
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
