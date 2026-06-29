type DashboardState = {
    mode: "loading" | "welcome" | "dashboard";
    currentPhase: {
        title: string;
        phaseId: string;
    } | null;
    plan: any;
};
export declare class DashboardService {
    private bridge;
    state: DashboardState | null;
    rfcs: any[];
    feedback: any[];
    constructor();
    get currentPhase(): {
        title: string;
        phaseId: string;
    } | null | undefined;
    get plan(): any;
    get mode(): "loading" | "welcome" | "dashboard";
    get isLoaded(): boolean;
}
export declare const dashboardService: DashboardService;
export {};
