import { expect, test } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { ApiMock, INSTANCES, OWNER } from "./api.fixture";

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

test("le dashboard affiche la santé opérationnelle et ouvre la gestion détaillée", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);

    await page.goto("/dashboard");
    await expect(page.getByText("Survie Valheim")).toBeVisible();
    await expect(page.getByText("Minecraft sans driver runtime")).toBeVisible();
    await expect(page.locator(".health-list .health-row")).toHaveCount(2);
    await expect(page.locator(".stat-pill").filter({ hasText: "en ligne" }).getByText("1", { exact: true })).toBeVisible();
    await expect(page.locator(".stat-pill").filter({ hasText: "À traiter" }).getByText("0", { exact: true })).toBeVisible();

    const restrictedRow = page.locator(".health-row").filter({ hasText: "Minecraft sans driver runtime" });
    await expect(restrictedRow.locator("button")).toHaveCount(0);

    await restrictedRow.click();
    await expect(page).toHaveURL(/\/servers\/22222222-2222-4222-8222-222222222222$/);
    await expect(page.locator(".server-tabs .tab-btn")).toHaveCount(2);
    await expect(page.locator(".server-tabs .tab-btn")).toHaveText(["Config", "Joueurs"]);
    await expect(page.getByRole("button", { name: "Installer" })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Démarrer" })).toHaveCount(0);
    await expect(page.getByRole("heading", { name: "Configuration dédiée du serveur" })).toBeVisible();
    await expect(page.getByText("Java téléchargé, vérifié et sélectionné automatiquement par DMX")).toBeVisible();
    await expect(page.getByRole("heading", { name: "Version et logiciel" })).toBeVisible();
    await expect(page.getByRole("heading", { name: "Réseau et ports" })).toBeVisible();
});

test("un serveur crashé garde son diagnostic visible et permet d’annuler le watchdog", async ({ page }) => {
    const crashedServer = {
        ...INSTANCES[0]!,
        desired_state: "running" as const,
        runtime_state: "crashed" as const,
    };
    const api = new ApiMock({ instances: [crashedServer, INSTANCES[1]!] });
    await api.install(page);

    await page.goto(`/servers/${crashedServer.id}`);
    await expect(page.getByText("Le processus du serveur s’est arrêté avec une erreur.")).toBeVisible();
    await expect(page.getByText(/Les logs et fichiers restent accessibles en lecture/)).toBeVisible();
    const cancelRestart = page.getByRole("button", { name: "Arrêter et annuler la relance" });
    await expect(cancelRestart).toBeVisible();
    await expect(page.getByRole("button", { name: "Démarrer" })).toHaveCount(0);
    await cancelRestart.click();

    expect(api.findRequest("POST", `/servers/${crashedServer.id}/actions/stop`)).toBeDefined();
});

test("la navigation principale se redimensionne au clavier et conserve sa largeur", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);
    await page.goto("/dashboard");

    await expect(page.getByRole("button", { name: /Réduire la navigation/ })).toHaveCount(0);
    const separator = page.getByRole("separator", { name: "Redimensionner la navigation" });
    await separator.focus();
    await expect(separator).toBeFocused();
    const focusStyle = await separator.evaluate((element) => {
        const style = getComputedStyle(element);
        return { boxShadow: style.boxShadow, outlineStyle: style.outlineStyle };
    });
    expect(focusStyle.boxShadow !== "none" || focusStyle.outlineStyle !== "none").toBe(true);
    await separator.press("End");
    await expect(separator).toHaveAttribute("aria-valuenow", "400");
    expect(await page.evaluate(() => localStorage.getItem(`dmx_sidebar_width:${"019f5c30-6557-7583-8d27-03a9cc043572"}`))).toBe("400");
    await page.reload();
    await expect(page.getByRole("separator", { name: "Redimensionner la navigation" })).toHaveAttribute("aria-valuenow", "400");

    await page.getByRole("link", { name: "Serveurs", exact: true }).focus();
    await page.getByRole("link", { name: "Serveurs", exact: true }).press("Enter");
    await expect(page).toHaveURL(/\/servers$/);
});

test("la vue liste ou grille des serveurs est conservée par utilisateur", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);
    await page.goto("/servers");
    await expect(page.locator(".server-grid")).toBeVisible();

    await page.getByRole("button", { name: "Affichage en liste" }).click();
    await expect(page.getByRole("table")).toBeVisible();
    await page.goto("/dashboard");
    await page.goto("/servers");
    await expect(page.getByRole("table")).toBeVisible();
    await page.reload();
    await expect(page.getByRole("table")).toBeVisible();
    expect(await page.evaluate(() => localStorage.getItem("dmx_server_view:019f5c30-6557-7583-8d27-03a9cc043572"))).toBe("list");
});

test("l’adresse de connexion reste masquée, se copie sans révélation et explique l’absence d’hôte", async ({ page }) => {
    const api = new ApiMock();
    await page.addInitScript(() => {
        Object.defineProperty(navigator, "clipboard", {
            configurable: true,
            value: {
                writeText: (value: string) => {
                    (window as typeof window & { __dmxCopiedAddress?: string }).__dmxCopiedAddress = value;
                    return Promise.resolve();
                },
            },
        });
    });
    await api.install(page);
    await page.goto(`/servers/${INSTANCES[0]!.id}`);

    const connection = page.locator(".connection-pill");
    await expect(connection).toContainText("••••••:2456");
    await expect(connection).not.toContainText("play.example.com:2456");
    await connection.getByRole("button", { name: "Copier l’adresse sans l’afficher" }).click();
    await expect(page.getByText("Adresse de connexion copiée.")).toBeVisible();
    expect(await page.evaluate(() => (window as typeof window & { __dmxCopiedAddress?: string }).__dmxCopiedAddress)).toBe("play.example.com:2456");
    await expect(connection).not.toContainText("play.example.com:2456");
    await connection.getByRole("button", { name: "Afficher l’adresse" }).click();
    await expect(connection).toContainText("play.example.com:2456");

    const withoutHost = new ApiMock();
    withoutHost.advertisedGameHost = null;
    await page.unrouteAll({ behavior: "ignoreErrors" });
    await withoutHost.install(page);
    await page.reload();
    await expect(page.getByText("Hôte public non configuré", { exact: true })).toBeVisible();
    await expect(page.getByRole("link", { name: "Configurer le réseau" })).toHaveAttribute("href", "/administration?tab=network");
});

test("Mon compte affiche et révoque les sessions sans exposer de jeton", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);
    await page.goto("/user-settings");

    await expect(page.getByRole("heading", { name: "Sessions actives" })).toBeVisible();
    await expect(page.getByText("Chromium")).toBeVisible();
    await expect(page.getByText("Safari")).toBeVisible();
    await expect(page.getByText("Session actuelle")).toBeVisible();
    expect(await page.locator("body").innerText()).not.toMatch(/token_hash|csrf_hash|user_agent|e2e-opaque-session-token/);

    await page.getByRole("button", { name: "Révoquer la session" }).click();
    const singleDialog = page.getByRole("dialog", { name: "Révoquer la session" });
    await singleDialog.getByRole("button", { name: "Révoquer la session" }).click();
    await expect(page.getByText("Safari · macOS")).toHaveCount(0);
    expect(api.findRequest("DELETE", "/auth/sessions/20202020-2020-4020-8020-202020202020")?.headers["x-csrf-token"]).toBe("e2e-csrf-token");

    await page.reload();
    await page.getByRole("button", { name: "Révoquer les autres" }).click();
    const othersDialog = page.getByRole("dialog", { name: "Révoquer les autres" });
    await othersDialog.getByRole("button", { name: "Révoquer les autres" }).click();
    await expect(page.getByText("Safari · macOS")).toHaveCount(0);
    expect(api.findRequest("POST", "/auth/sessions/revoke-others")?.headers["x-csrf-token"]).toBe("e2e-csrf-token");
});

test("la console reçoit server.log par SSE et échappe le HTML sans jeton dans l’URL", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);
    await page.goto("/servers/11111111-1111-4111-8111-111111111111");

    await page.getByRole("tab", { name: "Terminal" }).click();
    const output = page.locator(".console-output");
    await expect(output).toContainText("Serveur prêt <img src=x onerror=alert(1)>");
    await expect(output.locator("img")).toHaveCount(0);
    await page.getByRole("button", { name: "Copier tous les logs visibles" }).click();
    await expect(page.getByRole("button", { name: "Logs copiés" })).toBeVisible();

    const eventRequests = api.requests.filter((request) => request.path === "/events");
    expect(eventRequests[0]?.headers.cookie).toContain("dmx_session=e2e-opaque-session-token");
    expect(eventRequests[0]?.search).toBe("?server_id=11111111-1111-4111-8111-111111111111");
    expect(eventRequests[0]?.search).not.toMatch(/token|jwt/i);
    expect(eventRequests.every((request) => request.headers.authorization === undefined)).toBe(true);
});

test("la console rappelle les commandes avec les flèches haut et bas", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);
    const serverId = "11111111-1111-4111-8111-111111111111";
    await page.goto(`/servers/${serverId}?tab=console`);

    const input = page.getByPlaceholder("Entrez une commande...");
    await input.fill("status");
    await input.press("Enter");
    await input.fill("say maintenance dans 5 minutes");
    await input.press("Enter");

    await input.press("ArrowUp");
    await expect(input).toHaveValue("say maintenance dans 5 minutes");
    await input.press("ArrowUp");
    await expect(input).toHaveValue("status");
    await input.press("ArrowDown");
    await expect(input).toHaveValue("say maintenance dans 5 minutes");
    await input.press("ArrowDown");
    await expect(input).toHaveValue("");

    expect(api.requests.filter((request) => request.path === `/servers/${serverId}/console`).map((request) => request.body)).toEqual([
        { command: "status" },
        { command: "say maintenance dans 5 minutes" },
    ]);
});

test("l’onglet Joueurs affiche la présence et met les droits natifs en file", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);
    const serverId = "11111111-1111-4111-8111-111111111111";
    await page.goto(`/servers/${serverId}`);

    await page.getByRole("tab", { name: "Joueurs" }).click();
    await expect(page.getByText("DmxPlayer")).toBeVisible();
    await expect(page.getByText("adminlist.txt", { exact: true })).toBeVisible();
    await page.getByText("adminlist.txt", { exact: true }).click();
    await page.getByLabel("Entrées — adminlist.txt").fill("76561198000000000\n76561198000000001");
    await page.getByRole("button", { name: "Mettre cette liste en file" }).click();
    await expect(page.getByText("Modification chiffrée et mise en file.")).toBeVisible();
    await page.getByRole("button", { name: "Ouvrir l’éditeur avancé" }).click();
    const editorDialog = page.getByRole("dialog", { name: "adminlist.txt" });
    await expect(editorDialog.locator(".monaco-editor")).toBeVisible();

    const request = api.findRequest("PUT", `/servers/${serverId}/config-files/text`);
    expect(request?.body).toEqual({
        content: "76561198000000000\n76561198000000001",
        expected_sha256: "a".repeat(64),
    });
});

test("démarrer après une installation rebascule immédiatement le terminal sur la console serveur", async ({ page }) => {
    const stoppedServer = {
        ...INSTANCES[0]!,
        desired_state: "stopped" as const,
        runtime_state: "stopped" as const,
    };
    const api = new ApiMock({ instances: [stoppedServer, INSTANCES[1]!] });
    await api.install(page);
    const serverId = stoppedServer.id;
    await page.goto(`/servers/${serverId}?tab=console&source=install&job=dededede-dede-4ded-8ded-dededededede`);

    const output = page.locator(".console-output");
    await expect(output).toBeVisible();
    await expect(output).not.toContainText("Serveur prêt");
    await page.getByRole("button", { name: "Démarrer" }).click();

    await expect(page).toHaveURL(new RegExp(`/servers/${serverId}\\?tab=console$`));
    await expect(output).toContainText("Serveur prêt");
    expect(api.findRequest("POST", `/servers/${serverId}/actions/start`)).toBeDefined();
});
