import type {
  MachineChannelRequestEnvelope,
  MachineChannelResponseEnvelope,
} from "../../types/machineChannel";
import { MachineChannelServer } from "../../machine-channel/server";
import { getLogger } from "../../logging";
import type { ServerModeAvailability } from "../../machine-channel/DaemonChannelServer";

const logger = getLogger("lmtool");

function formatAvailabilityFailure(
  availability: Exclude<ServerModeAvailability, { available: true }>,
): string {
  if (availability.reason === "env-disabled") {
    return (
      `[machineChannel] Server mode is disabled by ${availability.envVar}=${availability.value}. ` +
      `workspace=${availability.workspaceRoot}.`
    );
  }

  return (
    "[machineChannel] Server mode is temporarily cooling down after " +
    `${availability.reconnectAttempts} failed reconnect attempts ` +
    `(max=${availability.maxReconnectAttempts}). ` +
    `retryAfterMs=${availability.retryAfterMs}; ` +
    `cooldownUntilMs=${availability.cooldownUntilMs}; ` +
    `workspace=${availability.workspaceRoot}. ` +
    "It will retry automatically after the cooldown, or run " +
    "'Exosuit: Restart Machine Channel Server' from the Command Palette to restart immediately."
  );
}

/**
 * Send a request to the exo machine channel.
 *
 * Uses the persistent server mode (RFC 0097). Errors are surfaced directly
 * rather than masked by fallback behavior (RFC 0135 Step 15).
 *
 * @throws Error if the machine channel server is unavailable or request fails
 */
export async function exoMachineChannel(
  cwd: string,
  request: MachineChannelRequestEnvelope,
): Promise<MachineChannelResponseEnvelope> {
  const server = MachineChannelServer.getInstance(cwd);

  const availability = server.getServerModeAvailability();
  if (!availability.available) {
    const error = new Error(formatAvailabilityFailure(availability));
    logger.error(
      "[machineChannel] Server mode unavailable:",
      availability,
      error,
    );
    throw error;
  }

  try {
    return await server.request(request);
  } catch (err) {
    // Surface errors loudly - no silent fallback (RFC 0135)
    logger.error("[machineChannel] Request failed:", err);
    throw err;
  }
}
