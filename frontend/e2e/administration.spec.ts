import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { UserInfoSchema } from "../src/schemas/api";
import { ApiMock, OWNER } from "./api.fixture";

test("l’Owner crée, réinitialise, désactive et affecte un compte sans exposer de secret", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);
    await page.goto("/administration");

    await expect(page.getByRole("heading", { name: "Administration" })).toBeVisible();
    await expect(page.getByRole("heading", { name: "Comptes locaux" })).toBeVisible();
    await page.getByRole("button", { name: "Créer un compte" }).click();
    await page.getByLabel("Nom d’utilisateur").fill("bob");
    await page.getByLabel("Mot de passe temporaire").fill("Secure-Bob-2026!");
    await page.getByLabel("Rôle").selectOption("viewer");
    await page.getByLabel("Langue").selectOption("en");
    await page.getByRole("button", { name: "Enregistrer le compte" }).click();

    const created = api.users.find((user) => user.username === "bob");
    expect(created).toBeDefined();
    await expect(page.getByRole("button", { name: /bob/ })).toBeVisible();
    const createRequest = api.findRequest("POST", "/users");
    expect(createRequest?.headers["x-csrf-token"]).toBe("e2e-csrf-token");
    expect(createRequest?.headers.authorization).toBeUndefined();
    expect(createRequest?.body).toEqual({
        username: "bob",
        password: "Secure-Bob-2026!",
        role_id: "viewer",
        language: "en",
    });
    expect(Object.hasOwn(created!, "password")).toBe(false);
    expect(Object.hasOwn(created!, "password_hash")).toBe(false);
    expect(Object.hasOwn(created!, "secret")).toBe(false);

    await page.getByLabel("Rôle").selectOption("operator");
    await page.getByLabel("Nouveau mot de passe temporaire").fill("Reset-Bob-2026!");
    await page.getByLabel(/Compte actif/).uncheck();
    await page.getByRole("button", { name: "Enregistrer le compte" }).click();

    await expect(page.getByRole("button", { name: /bob.*Désactivé/ })).toBeVisible();
    const updateRequest = api.findRequest("PATCH", `/users/${created!.id}`);
    expect(updateRequest?.headers["x-csrf-token"]).toBe("e2e-csrf-token");
    expect(updateRequest?.body).toEqual({
        role_id: "operator",
        is_active: false,
        language: "en",
        accent_color: "#3a82f6",
        password: "Reset-Bob-2026!",
    });

    await page.getByRole("button", { name: "Ajouter une affectation" }).click();
    await expect(page.getByLabel("Instance", { exact: true })).toHaveValue("11111111-1111-4111-8111-111111111111");
    await expect(page.getByLabel(/Utiliser les permissions du rôle/)).toBeChecked();
    await page.getByRole("button", { name: "Enregistrer l’affectation" }).click();

    const grantRequest = api.findRequest("PUT", `/users/${created!.id}/instances/11111111-1111-4111-8111-111111111111`);
    expect(grantRequest?.body).toEqual({ permissions: [] });
    expect(grantRequest?.headers["x-csrf-token"]).toBe("e2e-csrf-token");
    await expect(page.getByText("Toutes les permissions du rôle", { exact: true })).toBeVisible();

    await page.getByRole("button", { name: "Modifier l’affectation — Survie Valheim" }).click();
    await page.getByLabel(/Utiliser les permissions du rôle/).uncheck();
    await page.getByRole("checkbox", { name: /Consulter les instances/ }).check();
    await page.getByRole("button", { name: "Enregistrer l’affectation" }).click();
    expect(api.findRequest("PUT", `/users/${created!.id}/instances/11111111-1111-4111-8111-111111111111`)?.body)
        .toEqual({ permissions: ["server.read"] });

    page.once("dialog", (dialog) => dialog.accept());
    await page.getByRole("button", { name: "Retirer l’affectation — Survie Valheim" }).click();
    await expect(page.getByText("Aucune instance affectée.")).toBeVisible();
    expect(api.findRequest("DELETE", `/users/${created!.id}/instances/11111111-1111-4111-8111-111111111111`)).toBeDefined();
});

test("l’Owner gère un rôle personnalisé avec signalement des permissions à risque", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);
    await page.goto("/administration");

    const usersTab = page.getByRole("tab", { name: "Utilisateurs" });
    const rolesTab = page.getByRole("tab", { name: "Rôles" });
    await usersTab.focus();
    await page.keyboard.press("ArrowRight");
    await expect(rolesTab).toBeFocused();
    await expect(rolesTab).toHaveAttribute("aria-selected", "true");

    await page.getByRole("button", { name: "Créer un rôle" }).click();
    await page.getByLabel("Nom du rôle").fill("Modérateurs");
    await page.getByRole("checkbox", { name: /Consulter les instances/ }).check();
    const consolePermission = page.getByRole("checkbox", { name: /Écrire dans la console/ });
    await expect(consolePermission.locator("..")).toContainText("Risque élevé");
    await consolePermission.check();
    await page.locator("form").getByRole("button", { name: "Sauvegarder" }).click();

    await expect(page.getByRole("button", { name: /Modérateurs/ })).toBeVisible();
    const createRole = api.findRequest("POST", "/roles");
    expect(createRole?.body).toEqual({
        name: "Modérateurs",
        permissions: ["server.console.write", "server.read"],
    });
    expect(createRole?.headers["x-csrf-token"]).toBe("e2e-csrf-token");

    const customRole = api.roles.find((role) => role.name === "Modérateurs");
    expect(customRole).toBeDefined();
    await page.getByLabel("Nom du rôle").fill("Modérateurs jeu");
    await consolePermission.uncheck();
    await page.locator("form").getByRole("button", { name: "Sauvegarder" }).click();
    const updateRole = api.findRequest("PATCH", `/roles/${customRole!.id}`);
    expect(updateRole?.body).toEqual({ name: "Modérateurs jeu", permissions: ["server.read"] });
    await expect(page.getByRole("button", { name: /Modérateurs jeu/ })).toBeVisible();

    page.once("dialog", (dialog) => dialog.accept());
    await page.locator("form").getByRole("button", { name: "Supprimer" }).click();
    await expect(page.getByRole("button", { name: /Modérateurs jeu/ })).toHaveCount(0);
    expect(api.findRequest("DELETE", `/roles/${customRole!.id}`)?.headers["x-csrf-token"]).toBe("e2e-csrf-token");
});

test("l’Owner crée, révise et supprime un profil Steam versionné sans champ d’instance dangereux", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);
    await page.goto("/administration");
    await page.getByRole("tab", { name: "Profils Steam" }).click();
    await expect(page.getByRole("heading", { name: "Profils SteamCMD locaux" })).toBeVisible();

    await page.getByRole("button", { name: "Créer un profil", exact: true }).click();
    const form = page.locator("form.steam-profile-editor");
    await form.getByLabel("Identifiant technique").fill("steam-e2e");
    await form.getByLabel("Steam AppID").fill("654321");
    await form.getByLabel("Nom public").fill("Steam E2E");
    await form.getByLabel("Description").fill("Profil anonyme testé de bout en bout.");
    await form.getByLabel("Linux AMD64").fill("bin/server");
    await form.getByRole("button", { name: "Créer un profil", exact: true }).click();

    const create = api.findRequest("POST", "/game-profiles/steam");
    expect(create?.headers["x-csrf-token"]).toBe("e2e-csrf-token");
    expect(create?.body).toEqual({
        id: "steam-e2e",
        definition: {
            name: "Steam E2E",
            description: "Profil anonyme testé de bout en bout.",
            app_id: 654321,
            branch: null,
            executable: { linux_x86_64: "bin/server", windows_x86_64: null },
            arguments: [],
            ports: [{ name: "game", protocol: "udp", default: 27015, adjacent_to: null }],
            save_paths: ["saves"],
            ready_log_pattern: null,
            stop_strategy: { kind: "interrupt", timeout_seconds: 60 },
        },
    });
    await expect(page.getByRole("button", { name: /Steam E2E/ })).toBeVisible();

    await form.getByLabel("Nom public").fill("Steam E2E révisé");
    await form.getByRole("button", { name: "Créer la révision" }).click();
    const revision = api.findRequest("PUT", "/game-profiles/steam/steam-e2e");
    expect(revision?.headers["if-match"]).toBe('"1"');
    expect((revision?.body as { app_id?: number }).app_id).toBe(654321);

    page.once("dialog", (dialog) => dialog.accept());
    await form.getByRole("button", { name: "Supprimer", exact: true }).click();
    expect(api.findRequest("DELETE", "/game-profiles/steam/steam-e2e")?.headers["x-csrf-token"]).toBe("e2e-csrf-token");
    await expect(page.getByRole("button", { name: /Steam E2E révisé/ })).toHaveCount(0);
});

test("l’Owner gère un webhook Discord sans jamais relire ni persister son URL", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);
    await page.goto("/administration");
    await page.getByRole("tab", { name: "Webhooks Discord" }).click();

    await expect(page.getByRole("heading", { name: "Webhooks Discord" })).toBeVisible();
    await page.getByRole("button", { name: "Ajouter un webhook" }).click();
    const form = page.locator("form.webhook-editor");
    const secretUrl = "https://discord.com/api/webhooks/123456789012345678/abcdefghijklmnopqrstuvwxyzABCDEF0123456789";
    await form.getByLabel("Nom").fill("Incidents production");
    await form.getByLabel("URL Discord").fill(secretUrl);
    await form.getByRole("checkbox", { name: /Serveur planté/ }).check();
    await form.getByRole("button", { name: "Sauvegarder" }).click();

    const create = api.findRequest("POST", "/webhooks");
    expect(create?.headers["x-csrf-token"]).toBe("e2e-csrf-token");
    expect(create?.body).toEqual({
        name: "Incidents production",
        url: secretUrl,
        events: ["job.failed", "server.crashed"],
        enabled: true,
    });
    expect(Object.hasOwn(api.webhooks[0]!, "url")).toBe(false);
    await expect(page.getByText("URL configurée")).toBeVisible();
    await expect(page.getByText(secretUrl)).toHaveCount(0);
    expect(await page.evaluate(() => JSON.stringify(localStorage))).not.toContain(secretUrl);

    await page.getByRole("button", { name: "Modifier" }).click();
    await expect(form.getByLabel("Nouvelle URL Discord")).toHaveValue("");
    await form.getByLabel("Webhook actif").uncheck();
    await form.getByRole("button", { name: "Sauvegarder" }).click();
    const update = api.findRequest("PUT", `/webhooks/${api.webhooks[0]!.id}`);
    expect(update?.headers["if-match"]).toBe('"1"');
    expect(update?.body).toEqual({
        name: "Incidents production",
        events: ["job.failed", "server.crashed"],
        enabled: false,
    });

    page.once("dialog", (dialog) => dialog.accept());
    await page.getByRole("button", { name: "Supprimer Incidents production" }).click();
    expect(api.findRequest("DELETE", "/webhooks/13131313-1313-4313-8313-131313131313")).toBeDefined();
    await expect(page.getByText("Aucun webhook Discord configuré.")).toBeVisible();
});

test("l’Owner vérifie une release signée sans mise à jour automatique", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);
    await page.goto("/administration");
    await page.getByRole("tab", { name: "Mise à jour du panneau" }).click();

    const panel = page.locator("#administration-panel-releases");
    await expect(panel.getByRole("heading", { name: "Mise à jour du panneau" })).toBeVisible();
    await expect(panel.getByText("v1.0.0")).toBeVisible();
    await expect(panel.getByText("v1.0.1", { exact: true })).toBeVisible();
    await expect(panel.getByText("Signature et intégrité vérifiées")).toBeVisible();
    await expect(panel.getByText(/ne l’exécute pas et ne s’auto-remplace jamais/)).toBeVisible();
    await expect(panel.getByText("a".repeat(64), { exact: true })).toBeVisible();
    await expect(panel.getByText("b".repeat(64), { exact: true })).toBeVisible();
    await expect(panel.getByText(/DMX_EXPECTED_ARCHIVE_SHA256=/)).toBeVisible();

    await page.getByRole("button", { name: "Vérifier maintenant" }).click();
    const check = api.findRequest("POST", "/releases/panel/check");
    expect(check?.headers["x-csrf-token"]).toBe("e2e-csrf-token");
    expect(check?.headers.authorization).toBeUndefined();
    expect(check?.body).toBeNull();
});

test("aucune commande n’est proposée pour une release à jour ou une vérification échouée", async ({ page }) => {
    const verified = new ApiMock().releaseStatus;
    for (const releaseStatus of [
        { ...verified, state: "up_to_date" as const, latest: { ...verified.latest!, version: "1.0.0" } },
        { ...verified, state: "check_failed" as const, error_code: "network" as const },
    ]) {
        const api = new ApiMock({ releaseStatus });
        await api.install(page);
        await page.goto("/administration");
        await page.getByRole("tab", { name: "Mise à jour du panneau" }).click();

        const panel = page.locator("#administration-panel-releases");
        await expect(panel.getByRole("heading", { name: "Procédure de mise à niveau" })).toHaveCount(0);
        await expect(panel.getByText(/DMX_EXPECTED_ARCHIVE_SHA256=/)).toHaveCount(0);
        await page.unrouteAll({ behavior: "ignoreErrors" });
    }
});

test("l’Owner configure CurseForge sans jamais relire la clé API", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);
    await page.goto("/administration");
    await page.getByRole("tab", { name: "Fournisseurs de mods" }).click();

    await expect(page.getByRole("heading", { name: "Fournisseurs de mods" })).toBeVisible();
    await expect(page.getByText("Clé absente")).toBeVisible();
    const apiKey = "curseforge-e2e-secret-key-123456"; // gitleaks:allow
    const keyInput = page.getByLabel("Clé API CurseForge");
    await keyInput.fill(apiKey);
    await page.getByRole("button", { name: "Sauvegarder" }).click();

    const request = api.findRequest("PUT", "/mods/providers/curseforge");
    expect(request?.body).toEqual({ api_key: apiKey });
    expect(request?.headers["x-csrf-token"]).toBe("e2e-csrf-token");
    await expect(keyInput).toHaveValue("");
    await expect(page.getByText("Clé configurée")).toBeVisible();
    await expect(page.getByText(apiKey)).toHaveCount(0);
    expect(await page.evaluate(() => JSON.stringify(localStorage))).not.toContain(apiKey);
});

test("l’Admin ne gère ni l’Owner, ni les rôles, ni les définitions Steam à haut risque", async ({ page }) => {
    const admin = UserInfoSchema.parse({
        ...OWNER,
        id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        username: "admin",
        role: "admin",
        permissions: ["server.read", "server.create", "user.read", "user.create", "user.update", "profile.read"],
    });
    const api = new ApiMock({ user: admin });
    await api.install(page);
    await page.goto("/administration");

    await expect(page.getByRole("tab", { name: "Utilisateurs" })).toBeVisible();
    await expect(page.getByRole("tab", { name: "Rôles" })).toHaveCount(0);
    await expect(page.getByRole("tab", { name: "Profils Steam" })).toHaveCount(0);
    await expect(page.getByRole("tab", { name: "Fournisseurs de mods" })).toHaveCount(0);
    await expect(page.getByRole("tab", { name: "Webhooks Discord" })).toHaveCount(0);
    await expect(page.getByRole("tab", { name: "Mise à jour du panneau" })).toHaveCount(0);
    await expect(page.getByRole("button", { name: /owner/i })).toHaveCount(0);
    expect(api.findRequest("GET", "/permissions")).toBeUndefined();

    await page.goto("/servers/create");
    await expect(page.getByLabel("Profil de jeu").locator("option[value='steam-example']")).toHaveCount(1);
    await page.getByLabel("Profil de jeu").selectOption("steam-example");
    await expect(page.getByText(/AppID et exécutables ne sont pas modifiables/)).toBeVisible();
    await expect(page.getByLabel("Steam AppID")).toHaveCount(0);
    await expect(page.getByLabel("Linux AMD64")).toHaveCount(0);
});

test("un wildcard de domaine non autorisé par le backend ne donne aucun accès", async ({ page }) => {
    const viewer = UserInfoSchema.parse({
        ...OWNER,
        id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
        username: "viewer",
        role: "viewer",
        permissions: ["user.*"],
    });
    const api = new ApiMock({ user: viewer });
    await api.install(page);
    await page.goto("/administration");

    await expect(page.getByText("Vous n’avez pas la permission de consulter l’administration.")).toBeVisible();
    await expect(page.getByRole("link", { name: "Administration" })).toHaveCount(0);
    expect(api.findRequest("GET", "/users")).toBeUndefined();
});

test("l’administration reste responsive et conforme WCAG AA", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/administration");
    await expect(page.getByRole("heading", { name: "Comptes locaux" })).toBeVisible();

    const dimensions = await page.evaluate(() => ({
        viewport: window.innerWidth,
        document: document.documentElement.scrollWidth,
    }));
    expect(dimensions.document).toBeLessThanOrEqual(dimensions.viewport);

    const results = await new AxeBuilder({ page })
        .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"])
        .analyze();
    expect(results.violations, JSON.stringify(results.violations, null, 2)).toEqual([]);
});
