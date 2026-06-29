import { type Readable } from "svelte/store";
export declare class BridgeReceiver {
    private stores;
    constructor();
    private handleMessage;
    private getStore;
    use<T>(key: string, defaultValue?: T): Readable<T>;
}
