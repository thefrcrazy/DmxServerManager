import { expect, test } from "@playwright/test";
import { ApiMock } from "./api.fixture";

test("le premier Owner est créé avec un jeton d’installation et une session opaque", async ({ page }) => {
    const api = new ApiMock({ authenticated: false, needsSetup: true, instances: [] });
    const setupToken = "one-time-install-token-32-bytes-minimum";
    await api.install(page);

    await page.goto("/");
    await expect(page).toHaveURL(/\/setup$/);
    await expect(page.getByRole("heading", { name: "DmxServerManager" })).toBeVisible();
    await expect.poll(() => api.requests.filter((request) => request.method === "GET" && request.path === "/auth/status").length)
        .toBeGreaterThanOrEqual(2);

    const username = page.getByLabel("Nom d'utilisateur");
    const password = page.getByLabel("Mot de passe", { exact: true });
    const confirmation = page.getByLabel("Confirmer le mot de passe");
    const remoteToken = page.getByLabel("Jeton d’installation distant (optionnel)");
    await confirmation.fill("A-Strong-Owner-Passphrase-2026!");
    await remoteToken.fill(setupToken);
    // Fill the credential pair last: Chromium's password heuristics can
    // reconcile the first pair after another password input is populated.
    await username.fill("primary-owner");
    await password.fill("A-Strong-Owner-Passphrase-2026!");
    await expect(username).toHaveValue("primary-owner");
    await expect(password).toHaveValue("A-Strong-Owner-Passphrase-2026!");
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
