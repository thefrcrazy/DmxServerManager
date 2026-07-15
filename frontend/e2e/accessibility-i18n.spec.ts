import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { ApiMock } from "./api.fixture";

test("le changement FR/EN est immédiat et persiste sans donnée sensible", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);
    await page.goto("/user-settings");

    await expect(page.locator("html")).toHaveAttribute("lang", "fr");
    await expect(page.getByRole("heading", { name: "Paramètres Utilisateur" })).toBeVisible();
    await page.getByRole("button", { name: "🇺🇸 English" }).click();
    await expect(page.locator("html")).toHaveAttribute("lang", "en");
    await expect(page.getByRole("heading", { name: "User Settings" })).toBeVisible();
    await expect(page.getByRole("link", { name: "Servers" })).toBeVisible();

    await page.reload();
    await expect(page.locator("html")).toHaveAttribute("lang", "en");
    await expect(page.getByRole("heading", { name: "User Settings" })).toBeVisible();
    expect(await page.evaluate(() => localStorage.getItem("dmx_server_manager_language"))).toBe("en");
});

test.describe("responsive", () => {
    test.use({ viewport: { width: 390, height: 844 } });

    test("le dashboard mobile conserve son contenu sans débordement horizontal", async ({ page }) => {
        const api = new ApiMock();
        await api.install(page);
        await page.goto("/dashboard");

        await expect(page.getByText("Survie Valheim")).toBeVisible();
        await expect(page.getByRole("button", { name: "Menu utilisateur" })).toBeVisible();
        await expect.poll(() => api.findRequest("GET", "/health") !== undefined).toBe(true);
        await page.getByRole("button", { name: "Ouvrir la navigation" }).click();
        await expect(page.getByRole("button", { name: "Fermer la navigation" }).first()).toBeFocused();
        await expect(page.getByRole("link", { name: "Tableau de Bord" })).toBeVisible();
        await page.keyboard.press("Tab");
        await expect(page.getByRole("link", { name: "DmxServerManager" })).toBeFocused();
        await page.keyboard.press("Tab");
        await expect(page.getByRole("link", { name: "Tableau de Bord" })).toBeFocused();
        await page.keyboard.press("Tab");
        await expect(page.getByRole("link", { name: "Serveurs" })).toBeFocused();
        await page.keyboard.press("Enter");
        await expect(page).toHaveURL(/\/servers$/);
        const dimensions = await page.evaluate(() => ({
            viewport: window.innerWidth,
            document: document.documentElement.scrollWidth,
        }));
        expect(dimensions.document).toBeLessThanOrEqual(dimensions.viewport);
    });
});

test("les écrans de connexion et dashboard n’ont aucune violation WCAG A/AA axe", async ({ page }) => {
    const anonymousApi = new ApiMock({ authenticated: false });
    await anonymousApi.install(page);
    await page.goto("/login");
    await expect(page.getByRole("heading", { name: "DmxServerManager" })).toBeVisible();

    const loginResults = await new AxeBuilder({ page })
        .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"])
        .analyze();
    expect(loginResults.violations, JSON.stringify(loginResults.violations, null, 2)).toEqual([]);

    await page.getByLabel("Nom d'utilisateur").fill("owner");
    await page.getByLabel("Mot de passe").fill("Correct-Horse-2026!");
    await page.getByRole("button", { name: "Connexion" }).click();
    await expect(page).toHaveURL(/\/dashboard$/);

    const dashboardResults = await new AxeBuilder({ page })
        .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"])
        .analyze();
    expect(dashboardResults.violations, JSON.stringify(dashboardResults.violations, null, 2)).toEqual([]);
});
