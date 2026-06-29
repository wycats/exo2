import { writable } from "svelte/store";
export class BridgeReceiver {
    constructor() {
        this.stores = new Map();
        window.addEventListener("message", (event) => {
            this.handleMessage(event.data);
        });
    }
    handleMessage(message) {
        console.log('[BridgeReceiver] Received message:', message.type, message.payload?.key);
        if (message.type === "BRIDGE_SYNC") {
            const { key, value } = message.payload;
            this.getStore(key).set(value);
        }
    }
    getStore(key) {
        if (!this.stores.has(key)) {
            this.stores.set(key, writable(undefined));
        }
        return this.stores.get(key);
    }
    use(key, defaultValue) {
        const store = this.getStore(key);
        if (defaultValue !== undefined) {
            // Initialize with default if undefined
            store.update((v) => (v === undefined ? defaultValue : v));
        }
        return { subscribe: store.subscribe };
    }
}
