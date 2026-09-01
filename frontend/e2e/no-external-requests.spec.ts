import { expect, test, type Page, type Request } from "@playwright/test";
import { ApiMock, INSTANCES } from "./api.fixture";

// Game artwork is deliberately served by the publishers' CDNs; every other
// outbound origin is a defect. The advanced editor used to pull Monaco from
// cdn.jsdelivr.net, which the panel Content-Security-Policy blocks in production
// and which is unreachable on offline and LAN deployments.
const ALLOWED_EXTERNAL_HOSTS = new Set([
    "shared.akamai.steamstatic.com",
    "static-cdn.jtvnw.net",
]);

function collectForbiddenRequests(page: Page, baseURL: string): string[] {
    const forbidden: string[] = [];
    const origin = new URL(baseURL).origin;
    page.on("request", (request: Request) => {
        const url = request.url();
        if (url.startsWith("data:") || url.startsWith("blob:")) return;
        let host: string;
        try {
            const parsed = new URL(url);
            if (parsed.origin === origin) return;
            host = parsed.host;
        } catch {
            return;
        }
        if (ALLOWED_EXTERNAL_HOSTS.has(host)) return;
        forbidden.push(`${request.resourceType()} ${url}`);
    });
    return forbidden;
}

test("l’éditeur avancé ne contacte aucune origine externe", async ({ page, baseURL }) => {
    const forbidden = collectForbiddenRequests(page, baseURL!);
    const api = new ApiMock();
    await api.install(page);
    await page.setViewportSize({ width: 1301, height: 790 });
    await page.goto(`/servers/${INSTANCES[0]!.id}?tab=players`);

    await page.getByText("adminlist.txt", { exact: true }).click();
    await page.getByRole("button", { name: "Ouvrir l’éditeur avancé" }).click();
    await expect(page.getByRole("dialog", { name: "adminlist.txt" }).locator(".monaco-editor"))
        .toBeVisible({ timeout: 30_000 });

    expect(forbidden, `Requêtes sortantes interdites :\n${forbidden.join("\n")}`).toEqual([]);
});

test("la navigation principale ne contacte aucune origine externe", async ({ page, baseURL }) => {
    const forbidden = collectForbiddenRequests(page, baseURL!);
    const api = new ApiMock();
    await api.install(page);

    for (const route of ["/dashboard", "/servers", "/activity", "/administration", "/user-settings"]) {
        await page.goto(route);
        await page.waitForLoadState("networkidle").catch(() => undefined);
    }

    expect(forbidden, `Requêtes sortantes interdites :\n${forbidden.join("\n")}`).toEqual([]);
});
