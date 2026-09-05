import { defineConfig, devices } from "@playwright/test";

const port = 4173;
const baseURL = `http://127.0.0.1:${port}`;

export default defineConfig({
    testDir: "./e2e",
    fullyParallel: true,
    forbidOnly: Boolean(process.env.CI),
    retries: process.env.CI ? 2 : 0,
    workers: process.env.CI ? 1 : undefined,
    reporter: process.env.CI
        ? [["line"], ["html", { open: "never", outputFolder: "playwright-report" }]]
        : "list",
    timeout: 30_000,
    // 5 s suffisaient tant que la suite tournait à un worker. En local, où
    // Playwright ouvre autant de workers que de cœurs, le premier rendu d'une
    // vue qui charge un chunk paresseux de plusieurs mégaoctets dépasse ce
    // délai sous charge — un échec qui ne dit rien du code testé.
    expect: { timeout: 10_000 },
    use: {
        baseURL,
        locale: "fr-FR",
        timezoneId: "Europe/Paris",
        trace: "retain-on-failure",
        screenshot: "only-on-failure",
        video: "retain-on-failure",
    },
    projects: [
        // La suite complète tourne sur un seul moteur ; les projets appareil ne
        // rejouent que les garde-fous, pour ne pas quadrupler la durée de CI.
        {
            name: "chromium",
            use: { ...devices["Desktop Chrome"] },
            testIgnore: /device-guardrails\.spec\.ts/,
        },
        {
            // WebKit : le moteur où se manifestent le zoom iOS sur les champs
            // sous 16 px, `dvh` face à la barre d'URL et les zones de sécurité.
            name: "mobile-safari",
            use: { ...devices["iPhone 13"] },
            testMatch: /device-guardrails\.spec\.ts/,
        },
        {
            name: "mobile-chrome",
            use: { ...devices["Pixel 5"] },
            testMatch: /device-guardrails\.spec\.ts/,
        },
        {
            name: "tablet",
            use: { ...devices["iPad (gen 7)"] },
            testMatch: /device-guardrails\.spec\.ts/,
        },
    ],
    outputDir: "test-results",
    webServer: {
        command: `bun run dev --host 127.0.0.1 --port ${port}`,
        url: baseURL,
        reuseExistingServer: !process.env.CI,
        stdout: "pipe",
        stderr: "pipe",
        timeout: 120_000,
    },
});
