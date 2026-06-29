// Re-export DaemonChannelServer as MachineChannelServer for backward compatibility
// The DaemonChannelServer is the new socket-based implementation (RFC 0097)
export { DaemonChannelServer as MachineChannelServer } from "./DaemonChannelServer";
export { DaemonChannelServer } from "./DaemonChannelServer";

// Legacy stdio-based server (kept for reference, may be removed later)
export { MachineChannelServer as LegacyMachineChannelServer } from "../agent/lmtool/MachineChannelServer";
export {
  ensureDaemon,
  connectToSocket,
  daemonStatus,
  DaemonConnection,
  getSocketPath,
  getPidPath,
} from "./socket-client";
