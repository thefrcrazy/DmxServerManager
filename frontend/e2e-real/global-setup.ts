import { spawn, type ChildProcess } from "node:child_process";
import { constants, access, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import type { FullConfig } from "@playwright/test";
import { REAL_E2E_BASE_URL, REAL_E2E_PORT, REAL_E2E_SETUP_TOKEN } from "./runtime";

const MAX_LOG_BYTES = 128 * 1024;
const STARTUP_TIMEOUT_MS = 30_000;
const SHUTDOWN_TIMEOUT_MS = 10_000;

function waitForExit(child: ChildProcess, timeoutMs: number): Promise<boolean> {
    if (child.exitCode !== null || child.signalCode !== null) return Promise.resolve(true);
    return new Promise((resolveExit) => {
        const onExit = () => {
            clearTimeout(timer);
            resolveExit(true);
        };
        const timer = setTimeout(() => {
            child.off("exit", onExit);
            resolveExit(false);
        }, timeoutMs);
        child.once("exit", onExit);
    });
}

async function waitForHealth(child: ChildProcess): Promise<void> {
    const deadline = Date.now() + STARTUP_TIMEOUT_MS;
    let lastError = "health endpoint did not answer";
    while (Date.now() < deadline) {
        if (child.exitCode !== null || child.signalCode !== null) {
            throw new Error(`DmxServerManager exited during startup (${child.exitCode ?? child.signalCode}).`);
        }
        try {
            const response = await fetch(`${REAL_E2E_BASE_URL}/api/v1/health`, {
                signal: AbortSignal.timeout(1_000),
            });
            const payload = await response.json() as { status?: unknown; service?: unknown };
            if (response.ok && payload.status === "ok" && payload.service === "dmx-server-manager") return;
            lastError = `health returned HTTP ${response.status}`;
        } catch (error) {
            lastError = error instanceof Error ? error.message : String(error);
        }
        await delay(100);
    }
    throw new Error(`DmxServerManager did not become healthy: ${lastError}`);
}

export default async function globalSetup(_config: FullConfig): Promise<() => Promise<void>> {
    const frontendDirectory = resolve(import.meta.dirname, "..");
    const repositoryRoot = resolve(frontendDirectory, "..");
    const executable = resolve(
        repositoryRoot,
        "backend",
        "target",
        "debug",
        process.platform === "win32" ? "dmx-server-manager.exe" : "dmx-server-manager",
    );
    const staticDirectory = resolve(frontendDirectory, "dist");
    await access(executable, process.platform === "win32" ? constants.F_OK : constants.X_OK)
        .catch(() => {
            throw new Error("Real E2E backend is missing. Run `cargo build --locked --manifest-path ../backend/Cargo.toml`.");
        });
    await access(resolve(staticDirectory, "index.html"), constants.R_OK)
        .catch(() => {
            throw new Error("Real E2E SPA is missing. Run `bun run build`.");
        });

    const temporaryRoot = await mkdtemp(join(tmpdir(), "dmx-server-manager-real-e2e-"));
    let logs = "";
    const appendLog = (source: string, chunk: Buffer) => {
        logs += `[${source}] ${chunk.toString("utf8")}`;
        if (Buffer.byteLength(logs) > MAX_LOG_BYTES) logs = logs.slice(-MAX_LOG_BYTES);
    };
    const child = spawn(executable, [], {
        cwd: repositoryRoot,
        env: {
            ...process.env,
            DMX_BIND: `127.0.0.1:${REAL_E2E_PORT}`,
            DMX_CONFIG_FILE: resolve(temporaryRoot, "config.toml"),
            DMX_DATA_DIR: temporaryRoot,
            DMX_DEV_ORIGIN: REAL_E2E_BASE_URL,
            DMX_LOG: "dmx_server_manager=error,tower_http=error",
            DMX_SETUP_TOKEN: REAL_E2E_SETUP_TOKEN,
            DMX_STEAMCMD_PATH: resolve(temporaryRoot, "missing-steamcmd"),
            DMX_STATIC_DIR: staticDirectory,
        },
        stdio: ["ignore", "pipe", "pipe"],
        windowsHide: true,
    });
    child.stdout?.on("data", (chunk: Buffer) => appendLog("stdout", chunk));
    child.stderr?.on("data", (chunk: Buffer) => appendLog("stderr", chunk));

    let cleaned = false;
    const cleanup = async () => {
        if (cleaned) return;
        cleaned = true;
        if (child.exitCode === null && child.signalCode === null) child.kill("SIGTERM");
        if (!await waitForExit(child, SHUTDOWN_TIMEOUT_MS)) {
            child.kill("SIGKILL");
            await waitForExit(child, 2_000);
        }
        await rm(temporaryRoot, { recursive: true, force: true });
    };

    try {
        await waitForHealth(child);
    } catch (error) {
        await cleanup();
        const detail = error instanceof Error ? error.message : String(error);
        throw new Error(`${detail}\n${logs || "DmxServerManager produced no output."}`, { cause: error });
    }

    return cleanup;
}
