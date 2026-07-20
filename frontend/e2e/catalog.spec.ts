import { expect, test } from "@playwright/test";
import { ApiMock } from "./api.fixture";

test("l'Owner importe, suit, applique et supprime une révision .dmxpack exacte", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);
    await page.goto("/administration");
    await page.getByRole("tab", { name: "Catalogue" }).click();

    await expect(page.getByRole("heading", { name: "Catalogue local" })).toBeVisible();
    await expect(page.getByText("Midnight", { exact: true })).toBeVisible();

    await page.getByLabel("Paquet .dmxpack").setInputFiles({
        name: "theme-midnight.dmxpack",
        mimeType: "application/vnd.dmxpack+zip",
        buffer: Buffer.from("abc"),
    });
    await page.getByRole("button", { name: "Importer" }).click();
    await expect(page.getByText("Paquet importé et validé.")).toBeVisible();
    await expect(page.getByRole("progressbar")).toHaveAttribute("value", "100");

    const upload = api.findRequest("POST", "/catalog/import");
    expect(upload?.headers["x-dmx-package-sha256"])
        .toBe("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    expect(upload?.headers["idempotency-key"]).toMatch(/^catalog-ui-[0-9a-f-]{36}$/);
    expect(upload?.headers["content-type"]).toBe("application/vnd.dmxpack+zip");
    expect(upload?.headers["x-csrf-token"]).toBe("e2e-csrf-token");
    expect(upload?.headers.authorization).toBeUndefined();

    await page.getByRole("button", { name: /Midnight/ }).click();
    await expect(page.locator(".catalog-revision-list").getByText("Version 1", { exact: true })).toBeVisible();
    await page.getByRole("button", { name: "Appliquer ce thème" }).click();
    await expect(page.getByText("Thème global appliqué.")).toBeVisible();
    await expect.poll(() => page.evaluate(() => (
        document.documentElement.style.getPropertyValue("--color-bg-primary")
    ))).toBe("#080B14");

    const catalogThemeRequests = api.requests.filter((request) => (
        request.method === "PUT" && request.path === "/catalog/theme"
    ));
    expect(catalogThemeRequests[0]?.headers["if-match"]).toBe('"1"');
    expect(catalogThemeRequests[0]?.headers["x-csrf-token"]).toBe("e2e-csrf-token");
    expect(catalogThemeRequests[0]?.body).toEqual({
        kind: "catalog",
        package_id: "theme-midnight",
        revision: 1,
    });

    await page.getByRole("button", { name: "Rétablir le thème par défaut" }).click();
    await expect.poll(() => page.evaluate(() => (
        document.documentElement.style.getPropertyValue("--color-bg-primary")
    ))).toBe("#000000");
    expect(api.findRequest("PUT", "/catalog/theme")?.headers["if-match"]).toBe('"2"');
    expect(api.findRequest("PUT", "/catalog/theme")?.body).toEqual({ kind: "default" });

    await page.getByRole("button", { name: "Supprimer", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "Supprimer la version" });
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: "Supprimer", exact: true }).click();
    await expect(page.getByText("Version supprimée.")).toBeVisible();
    expect(api.findRequest("DELETE", "/catalog/theme/theme-midnight/revisions/1")?.headers["x-csrf-token"])
        .toBe("e2e-csrf-token");
    await expect(page.getByText("Aucun paquet n’est installé.", { exact: true })).toBeVisible();
});
