import { expect, test } from "@playwright/test";
import { ApiMock } from "./api.fixture";

test("le premier Owner est créé avec un jeton d’installation et une session opaque", async ({ page }) => {
    const api = new ApiMock({ authenticated: false, needsSetup: true, instances: [] });
    const setupToken = "one-time-install-token-32-bytes-minimum";
    await api.install(page);

    await page.goto("/");
    await expect(page).toHaveURL(/\/setup$/);
    await expect(page.getByRole("heading", { name: "DmxServerManager" })).toBeVisible();

    const username = page.getByLabel("Nom d'utilisateur");
    await username.fill("primary-owner");
    await expect(username).toHaveValue("primary-owner");
    await page.getByLabel("Mot de passe", { exact: true }).fill("A-Strong-Owner-Passphrase-2026!");
    await page.getByLabel("Confirmer le mot de passe").fill("A-Strong-Owner-Passphrase-2026!");
    await page.getByLabel("Jeton d’installation distant (optionnel)").fill(setupToken);
    await Promise.all([
        page.waitForURL(/\/dashboard$/, { timeout: 10_000 }),
        page.getByRole("button", { name: "Terminer l'installation" }).click(),
    ]);
    await expect(page.getByRole("region", { name: "Vue d’ensemble opérationnelle" })).toBeVisible();

    const setup = api.findRequest("POST", "/auth/setup");
    expect(setup).toBeDefined();
    expect(setup?.headers["x-setup-token"]).toBe(setupToken);
    expect(setup?.headers.authorization).toBeUndefined();
    expect(setup?.body).toEqual({
        username: "primary-owner",
        password: "A-Strong-Owner-Passphrase-2026!",
    });

    const sessionCookies = await page.context().cookies();
    expect(sessionCookies).toContainEqual(expect.objectContaining({
        name: "dmx_session",
        httpOnly: true,
        sameSite: "Strict",
        path: "/api/v1",
    }));
    expect(api.requests.every((request) => request.headers.authorization === undefined)).toBe(true);
});
