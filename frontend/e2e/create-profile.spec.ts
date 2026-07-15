import { expect, test } from "@playwright/test";
import { ApiMock, ROLES } from "./api.fixture";

test("la création pilotée par profil sépare les secrets et envoie cookie plus CSRF", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);

    await page.goto("/servers/create");
    await expect(page.getByRole("heading", { name: "Nouveau serveur" })).toBeVisible();

    await page.getByLabel("Profil de jeu").selectOption("valheim");
    await page.getByLabel("Nom du serveur").fill("Valheim des amis");
    await page.getByLabel("Nom public").fill("Valheim Public");
    await page.getByLabel("Monde").fill("DmxWorld");
    await page.getByLabel("Mot de passe serveur").fill("Secret-Only-In-Vault");
    await page.getByLabel("Crossplay").check();
    await page.getByText("Démarrer automatiquement après les redémarrages du panneau").click();
    await page.getByRole("button", { name: "Créer et installer" }).click();

    await expect(page).toHaveURL(/\/jobs\?focus=44444444-4444-4444-8444-444444444444&instance=33333333-3333-4333-8333-333333333333$/);
    await expect(page.getByText("Installation / mise à jour")).toBeVisible();

    const creation = api.findRequest("POST", "/servers");
    expect(creation).toBeDefined();
    expect(creation?.headers["x-csrf-token"]).toBe("e2e-csrf-token");
    expect(creation?.headers.cookie).toContain("dmx_session=e2e-opaque-session-token");
    expect(creation?.headers.authorization).toBeUndefined();
    expect(creation?.body).toEqual({
        name: "Valheim des amis",
        profile_id: "valheim",
        settings: {
            server_name: "Valheim Public",
            world_name: "DmxWorld",
            port: 2456,
            query_port: 2457,
            crossplay: true,
        },
        secrets: { server_password: "Secret-Only-In-Vault" },
        auto_start: true,
    });
    expect(api.findRequest("POST", "/servers/33333333-3333-4333-8333-333333333333/actions/install")).toBeDefined();
});

test("Minecraft refuse la création tant que l’EULA n’est pas acceptée", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);
    await page.goto("/servers/create");

    await page.getByLabel("Nom du serveur").fill("Minecraft Vanilla");
    await page.getByRole("button", { name: "Créer et installer" }).click();
    const eula = page.getByRole("checkbox", { name: "J’accepte le contrat EULA" });
    await expect(eula).toBeFocused();
    expect(await eula.evaluate((element: HTMLInputElement) => element.validity.valueMissing)).toBe(true);
    expect(api.findRequest("POST", "/servers")).toBeUndefined();
});

test("importe une archive ZIP brute dans le stockage géré", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);
    await page.goto("/servers/create");

    await page.getByRole("button", { name: /Archive ZIP/ }).click();
    await page.getByLabel("Nom du serveur").fill("Minecraft importé");
    await page.getByRole("checkbox", { name: "J’accepte le contrat EULA" }).check();
    await page.getByLabel("Choisir l’archive ZIP du serveur").setInputFiles({
        name: "minecraft.zip",
        mimeType: "application/zip",
        buffer: Buffer.from([0x50, 0x4b, 0x03, 0x04, 0x00, 0x00]),
    });
    await page.getByRole("button", { name: "Créer et importer le ZIP" }).click();

    await expect(page).toHaveURL(/\/jobs\?/);
    const request = api.findRequest("POST", "/servers/33333333-3333-4333-8333-333333333333/imports/zip");
    expect(request?.headers["content-type"]).toContain("application/zip");
    expect(request?.headers["x-csrf-token"]).toBe("e2e-csrf-token");
    expect(request?.headers["idempotency-key"]).toMatch(/^[0-9a-f-]{36}$/i);
});

for (const mode of [
    { button: /Copier un dossier/, submit: "Créer et copier", endpoint: "copy" },
    { button: /Attacher un dossier/, submit: "Créer et attacher", endpoint: "attach" },
] as const) {
    test(`${mode.endpoint} un dossier déclaré sans choisir d’exécutable arbitraire`, async ({ page }) => {
        const api = new ApiMock();
        await api.install(page);
        await page.goto("/servers/create");

        await page.getByRole("button", { name: mode.button }).click();
        await page.getByLabel("Nom du serveur").fill(`Minecraft ${mode.endpoint}`);
        await page.getByRole("checkbox", { name: "J’accepte le contrat EULA" }).check();
        await page.getByLabel("Chemin source").fill(`/imports/minecraft-${mode.endpoint}`);
        await page.getByRole("button", { name: mode.submit }).click();

        await expect(page).toHaveURL(/\/jobs\?/);
        const request = api.findRequest("POST", `/servers/33333333-3333-4333-8333-333333333333/imports/${mode.endpoint}`);
        expect(request?.body).toEqual({ source_path: `/imports/minecraft-${mode.endpoint}` });
        expect(request?.headers["idempotency-key"]).toMatch(/^[0-9a-f-]{36}$/i);
    });
}

test("le mode attach reste masqué pour un Admin même avec l’écriture fichiers", async ({ page }) => {
    const adminRole = ROLES.find((role) => role.id === "admin")!;
    const api = new ApiMock({
        user: {
            id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            username: "admin",
            role: "admin",
            permissions: adminRole.permissions,
            accent_color: "#3a82f6",
            must_change_password: false,
        },
    });
    await api.install(page);
    await page.goto("/servers/create");

    await expect(page.getByRole("button", { name: /Attacher un dossier/ })).toHaveCount(0);
    await expect(page.getByRole("button", { name: /Copier un dossier/ })).toBeVisible();
});
