const configuredPort = Number.parseInt(process.env.DMX_REAL_E2E_PORT ?? "4287", 10);

if (!Number.isInteger(configuredPort) || configuredPort < 1024 || configuredPort > 65_535) {
    throw new Error("DMX_REAL_E2E_PORT must be an integer between 1024 and 65535.");
}

export const REAL_E2E_PORT = configuredPort;
export const REAL_E2E_BASE_URL = `http://127.0.0.1:${REAL_E2E_PORT}`;
export const REAL_E2E_SETUP_TOKEN = "dmx-real-e2e-one-time-setup-token";
