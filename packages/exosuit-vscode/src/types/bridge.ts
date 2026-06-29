export type BridgeMessage<T> = {
  type: "BRIDGE_SYNC";
  payload: {
    key: string;
    value: T;
    version: number;
  };
};

export type BridgeHandshake = {
  type: "BRIDGE_HELLO";
};
