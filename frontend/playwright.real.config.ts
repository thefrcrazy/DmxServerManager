import { defineConfig, devices } from "@playwright/test";
import { REAL_E2E_BASE_URL } from "./e2e-real/runtime";

export default defineConfig({
    testDir: "./e2e-real",
    testMatch: "real-stack.spec.ts",
    fullyParallel: false,
    forbidOnly: true,
    retries: 0,
    workers: 1,
    reporter: process.env.CI
        ? [["line"], ["html", { open: "never", outputFolder: "playwright-report-real" }]]
        : "list",
    timeout: 45_000,
    expect: { timeout: 8_000 },
    globalSetup: "./e2e-real/global-setup.ts",
    use: {
        ...devices["Desktop Chrome"],
        baseURL: REAL_E2E_BASE_URL,
        locale: "fr-FR",
        timezoneId: "Europe/Paris",
        trace: "retain-on-failure",
        screenshot: "only-on-failure",
        video: "retain-on-failure",
    },
    outputDir: "test-results-real",
});
