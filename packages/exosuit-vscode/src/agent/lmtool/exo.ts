import { spawn } from "node:child_process";
import { resolveExoBinary } from "../../exoBin";

export interface ExoJsonOptions {
  cwd: string;
  args: string[];
}

export async function exoJson(options: ExoJsonOptions): Promise<unknown> {
  const stdout = await exoExec({
    cwd: options.cwd,
    args: [...options.args, "--format", "json"],
  });

  try {
    return JSON.parse(stdout);
  } catch (e) {
    throw new Error(
      `Failed to parse exo JSON output: ${
        e instanceof Error ? e.message : String(e)
      }\n\nOutput:\n${stdout}`,
    );
  }
}

export async function exoExec(options: ExoJsonOptions): Promise<string> {
  return new Promise((resolve, reject) => {
    const child = spawn(resolveExoBinary("exo", options.cwd), options.args, {
      cwd: options.cwd,
      stdio: ["ignore", "pipe", "pipe"],
    });

    let stdout = "";
    let stderr = "";

    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");

    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });

    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });

    child.on("error", (err) => {
      reject(err);
    });

    child.on("close", (code) => {
      if (code === 0) {
        resolve(stdout.trim());
      } else {
        reject(
          new Error(
            `exo exited with code ${
              code ?? "<unknown>"
            }\n\nstdout:\n${stdout}\n\nstderr:\n${stderr}`,
          ),
        );
      }
    });
  });
}
