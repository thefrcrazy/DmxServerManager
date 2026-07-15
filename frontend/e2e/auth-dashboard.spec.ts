import { expect, test } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { ApiMock, OWNER } from "./api.fixture";

test("la connexion, le CSRF et la révocation de session fonctionnent sans JWT navigateur", async ({ page }) => {
    const api = new ApiMock({ authenticated: false });
    await api.install(page);

    await page.goto("/dashboard");
    await expect(page).toHaveURL(/\/login$/);

    const username = page.getByLabel("Nom d'utilisateur");
    const password = page.getByLabel("Mot de passe");
    await expect(username).toBeFocused();
    await username.fill("owner");
    await page.keyboard.press("Tab");
    await expect(password).toBeFocused();
    await password.fill("Correct-Horse-2026!");
    await page.keyboard.press("Tab");
    await expect(page.getByRole("button", { name: "Connexion" })).toBeFocused();
    await page.keyboard.press("Enter");

    await expect(page).toHaveURL(/\/dashboard$/);
    await expect(page.getByRole("heading", { name: "Tableau de Bord" })).toBeVisible();

    const login = api.findRequest("POST", "/auth/login");
    expect(login?.body).toEqual({ username: "owner", password: "Correct-Horse-2026!" });
    expect(login?.headers.authorization).toBeUndefined();

    await page.getByRole("button", { name: "Menu utilisateur" }).click();
    await page.getByRole("button", { name: "Déconnexion" }).click();
    await expect(page).toHaveURL(/\/login$/);

    const logout = api.findRequest("POST", "/auth/logout");
    expect(logout?.headers["x-csrf-token"]).toBe("e2e-csrf-token");
    expect(logout?.headers.cookie).toContain("dmx_session=e2e-opaque-session-token");
    expect(await page.context().cookies()).toHaveLength(0);
    expect(await page.evaluate(() => Object.keys(localStorage).filter((key) => /auth|jwt|token/i.test(key)))).toEqual([]);
});

test("un mot de passe temporaire bloque le panneau jusqu’à son remplacement", async ({ page }) => {
    const api = new ApiMock({
        authenticated: false,
        user: { ...OWNER, must_change_password: true },
    });
    await api.install(page);

    await page.goto("/dashboard");
    await page.getByLabel("Nom d'utilisateur").fill("owner");
    await page.getByLabel("Mot de passe").fill("Correct-Horse-2026!");
    await page.getByRole("button", { name: "Connexion" }).click();

    await expect(page).toHaveURL(/\/change-password$/);
    await expect(page.getByRole("heading", { name: "Changement de mot de passe requis" })).toBeVisible();
    await expect(page.getByLabel("Mot de passe actuel")).toBeFocused();
    await expect(page.getByRole("navigation")).toHaveCount(0);

    await page.goto("/servers");
    await expect(page).toHaveURL(/\/change-password$/);
    const protectedResponse = await page.evaluate(async () => {
        const response = await fetch("/api/v1/game-profiles");
        return { status: response.status, body: await response.json() as unknown };
    });
    expect(protectedResponse.status).toBe(403);
    expect(protectedResponse.body).toEqual(expect.objectContaining({ code: "AUTH_009" }));

    const accessibility = await new AxeBuilder({ page })
        .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
        .analyze();
    expect(accessibility.violations).toEqual([]);

    await page.getByLabel("Mot de passe actuel").fill("Correct-Horse-2026!");
    await page.getByLabel("Nouveau mot de passe").fill("Permanent-Owner-2026!");
    await page.getByLabel("Confirmer le mot de passe").fill("Permanent-Owner-2026!");
    await page.getByRole("button", { name: "Définir mon nouveau mot de passe" }).click();

    await expect(page).toHaveURL(/\/login$/);
    await expect(page.getByText("Mot de passe modifié. Reconnectez-vous avec votre nouveau mot de passe.")).toBeVisible();
    expect(api.findRequest("PUT", "/auth/password")).toEqual(expect.objectContaining({
        body: {
            current_password: "Correct-Horse-2026!",
            new_password: "Permanent-Owner-2026!",
        },
        headers: expect.objectContaining({ "x-csrf-token": "e2e-csrf-token" }),
    }));
    expect(await page.context().cookies()).toHaveLength(0);
});

test("le dashboard affiche les instances et masque les actions absentes des capabilities", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);

    await page.goto("/dashboard");
    await expect(page.getByText("Survie Valheim")).toBeVisible();
    await expect(page.getByText("Minecraft sans driver runtime")).toBeVisible();
    await expect(page.getByText("Total Serveurs").locator("..").getByText("2", { exact: true })).toBeVisible();

    const valheimRow = page.getByRole("row").filter({ hasText: "Survie Valheim" });
    await expect(valheimRow.locator("button")).toHaveCount(3);

    const restrictedRow = page.getByRole("row").filter({ hasText: "Minecraft sans driver runtime" });
    await expect(restrictedRow.locator("button")).toHaveCount(0);
    await expect(restrictedRow.getByText("—", { exact: true })).toBeVisible();

    await restrictedRow.click();
    await expect(page).toHaveURL(/\/servers\/22222222-2222-4222-8222-222222222222$/);
    await expect(page.locator(".server-tabs .tab-btn")).toHaveCount(1);
    await expect(page.locator(".server-tabs .tab-btn")).toHaveText("Config");
    await expect(page.getByRole("button", { name: "Installer" })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Démarrer" })).toHaveCount(0);
});

test("la navigation principale reste utilisable au clavier avec un focus visible", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);
    await page.goto("/dashboard");

    const collapse = page.getByRole("button", { name: "Réduire la navigation" });
    await collapse.focus();
    await expect(collapse).toBeFocused();
    const focusStyle = await collapse.evaluate((element) => {
        const style = getComputedStyle(element);
        return { boxShadow: style.boxShadow, outlineStyle: style.outlineStyle };
    });
    expect(focusStyle.boxShadow !== "none" || focusStyle.outlineStyle !== "none").toBe(true);

    await page.keyboard.press("Tab");
    await expect(page.getByRole("link", { name: "DmxServerManager" })).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(page.getByRole("link", { name: "Tableau de Bord" })).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(page.getByRole("link", { name: "Serveurs" })).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(page).toHaveURL(/\/servers$/);
});

test("la console reçoit server.log par SSE et échappe le HTML sans jeton dans l’URL", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);
    await page.goto("/servers/11111111-1111-4111-8111-111111111111");

    await page.getByRole("tab", { name: "Terminal" }).click();
    const output = page.locator(".console-output");
    await expect(output).toContainText("Serveur prêt <img src=x onerror=alert(1)>");
    await expect(output.locator("img")).toHaveCount(0);

    const eventRequests = api.requests.filter((request) => request.path === "/events");
    expect(eventRequests[0]?.headers.cookie).toContain("dmx_session=e2e-opaque-session-token");
    expect(eventRequests[0]?.search).toBe("?server_id=11111111-1111-4111-8111-111111111111");
    expect(eventRequests[0]?.search).not.toMatch(/token|jwt/i);
    expect(eventRequests.every((request) => request.headers.authorization === undefined)).toBe(true);
});
