import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import { expect, test } from "@playwright/test";
import { JobSchema } from "../src/schemas/api";
import { ApiMock, INSTANCES, OWNER } from "./api.fixture";

const captureEnabled = process.env.DMX_CAPTURE_VISUAL_REFERENCES === "1";
const outputDirectory = resolve(process.cwd(), "../docs/visual-references/v1.1.0");

test.describe("références visuelles v1.1.0", () => {
    test.skip(!captureEnabled, "Capture manuelle via bun run capture:visuals");
    test.describe.configure({ mode: "serial" });

    test("capture les écrans desktop et mobile déterministes", async ({ page }) => {
        const failedJob = JobSchema.parse({
            id: "56565656-5656-4565-8565-565656565656",
            instance_id: INSTANCES[0]!.id,
            kind: "restart",
            state: "failed",
            progress: 100,
            requested_by: OWNER.id,
            created_at: "2026-07-13T12:00:00.000Z",
            started_at: "2026-07-13T12:00:00.000Z",
            finished_at: "2026-07-13T12:01:00.000Z",
            error_code: "runtime.failed",
            error_message: "Le processus a quitté.",
            interaction: null,
        });
        const runningJob = JobSchema.parse({
            id: "efefefef-efef-4fef-8fef-efefefefefef",
            instance_id: INSTANCES[1]!.id,
            kind: "install",
            state: "running",
            progress: 42,
            requested_by: OWNER.id,
            created_at: "2026-07-13T12:02:00.000Z",
            started_at: "2026-07-13T12:03:00.000Z",
            interaction: null,
        });
        const api = new ApiMock({ jobs: [runningJob, failedJob] });
        await api.install(page);
        await page.emulateMedia({ reducedMotion: "reduce", colorScheme: "dark" });
        await mkdir(outputDirectory, { recursive: true });

        const capture = async (
            filename: string,
            url: string,
            viewport: { width: number; height: number },
            ready: () => Promise<void>,
        ) => {
            await page.setViewportSize(viewport);
            await page.goto(url);
            await ready();
            await page.waitForFunction(() => Array.from(document.images).every((image) => image.complete));
            await page.screenshot({
                path: resolve(outputDirectory, filename),
                animations: "disabled",
                caret: "hide",
                scale: "css",
            });
        };

        const desktop = { width: 1420, height: 1076 };
        await capture("01-dashboard-desktop.png", "/dashboard", desktop, async () => {
            await expect(page.getByRole("region", { name: "Vue d’ensemble opérationnelle" })).toBeVisible();
        });
        await capture("02-servers-desktop.png", "/servers", desktop, async () => {
            await expect(page.getByRole("heading", { name: "Serveurs" })).toBeVisible();
        });
        await capture("03-activity-operations-desktop.png", "/activity?tab=operations", desktop, async () => {
            await expect(page.getByRole("tab", { name: "Opérations" })).toHaveAttribute("aria-selected", "true");
        });
        await capture("04-activity-journal-desktop.png", "/activity?tab=journal", desktop, async () => {
            await expect(page.getByText("server.updated", { exact: true })).toBeVisible();
        });
        await capture("05-account-desktop.png", "/user-settings", desktop, async () => {
            await expect(page.getByRole("heading", { name: "Sessions actives" })).toBeVisible();
        });
        await capture("06-server-configuration-desktop.png", `/servers/${INSTANCES[0]!.id}?tab=config`, { width: 1385, height: 545 }, async () => {
            await expect(page.getByRole("heading", { name: "Configuration dédiée du serveur" })).toBeVisible();
        });

        await page.setViewportSize({ width: 1301, height: 790 });
        await page.goto(`/servers/${INSTANCES[0]!.id}?tab=players`);
        await page.getByText("adminlist.txt", { exact: true }).click();
        await page.getByRole("button", { name: "Modifier" }).click();
        await expect(page.getByRole("dialog", { name: "adminlist.txt" }).locator(".monaco-editor")).toBeVisible();
        await page.screenshot({
            path: resolve(outputDirectory, "07-native-editor-desktop.png"),
            animations: "disabled",
            caret: "hide",
            scale: "css",
        });

        await capture("08-dashboard-mobile.png", "/dashboard", { width: 390, height: 844 }, async () => {
            await expect(page.getByRole("region", { name: "Vue d’ensemble opérationnelle" })).toBeVisible();
        });
    });
});
