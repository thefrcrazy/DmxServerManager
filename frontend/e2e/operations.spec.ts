import { expect, test } from "@playwright/test";
import { GameProfileSchema, JobSchema, UserInfoSchema } from "../src/schemas/api";
import { ApiMock, GAME_PROFILES, INSTANCES, OWNER } from "./api.fixture";

test("les anciennes routes redirigent vers Activité sans appeler les API supprimées", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);

    await page.goto("/chat");
    await expect(page).toHaveURL(/\/activity$/);
    await expect(page.getByRole("tab", { name: "À traiter" })).toBeVisible();

    await page.goto("/notifications");
    await expect(page).toHaveURL(/\/activity\?tab=attention$/);

    await page.goto("/jobs");
    await expect(page).toHaveURL(/\/activity\?tab=operations$/);
    expect(api.findRequest("GET", "/chat")).toBeUndefined();
    expect(api.findRequest("GET", "/notifications")).toBeUndefined();
});

test("Activité sépare les incidents, les opérations et le journal réservé", async ({ page }) => {
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
    const api = new ApiMock({ jobs: [failedJob] });
    await api.install(page);

    await page.goto("/activity");
    await expect(page.getByText("Le processus a quitté.")).toHaveCount(0);
    await page.getByRole("button", { name: /Redémarrage/ }).click();
    await expect(page.getByText("Le processus a quitté.")).toBeVisible();
    await expect(page.locator(".activity-drawer-backdrop")).toHaveClass(/is-open/);
    await page.getByRole("button", { name: "Fermer" }).click();
    await expect(page.locator(".activity-drawer-backdrop")).not.toHaveClass(/is-open/);
    await expect(page.locator(".activity-drawer-backdrop")).toHaveCount(0);

    await page.getByRole("tab", { name: "Opérations" }).click();
    await expect(page).toHaveURL(/tab=operations/);
    await expect(page.getByRole("button", { name: /Redémarrage/ })).toBeVisible();

    await page.getByRole("tab", { name: "Journal" }).click();
    await expect(page.getByText("server.updated", { exact: true })).toBeVisible();
    expect(api.findRequest("GET", "/audit")).toBeDefined();
});

test("la configuration native expose un formulaire sûr puis conserve le contenu inconnu", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);
    const serverId = INSTANCES[0]!.id;
    await page.goto(`/servers/${serverId}?tab=configuration`);

    await page.getByText("config.json", { exact: true }).click();
    await expect(page.getByRole("region", { name: "Réglages principaux" })).toBeVisible();
    await expect(page.getByText("Hytale Server", { exact: true })).toBeVisible();
    await expect(page.getByText("Non configuré", { exact: true })).toBeVisible();

    await page.getByRole("button", { name: "Modifier", exact: true }).click();
    await page.getByLabel("ServerName", { exact: true }).fill("Hytale de Max");
    await page.getByLabel("MaxPlayers", { exact: true }).fill("48");
    await page.getByRole("button", { name: "Mettre les réglages en file" }).click();

    const request = api.findRequest("PUT", `/servers/${serverId}/config-files/text`);
    expect(new URLSearchParams(request?.search).get("path")).toBe("game/Server/config.json");
    expect(request?.body).toMatchObject({ expected_sha256: "c".repeat(64) });
    const queued = JSON.parse((request?.body as { content: string }).content);
    expect(queued.ServerName).toBe("Hytale de Max");
    expect(queued.MaxPlayers).toBe(48);
    expect(queued.Modules).toEqual({});
});

test("le Journal reste masqué pour un rôle non administrateur même si une permission obsolète subsiste", async ({ page }) => {
    const api = new ApiMock({
        user: UserInfoSchema.parse({
            ...OWNER,
            id: "34343434-3434-4434-8434-343434343434",
            username: "legacy-auditor",
            role: "operator",
            permissions: ["server.read", "job.read", "audit.read"],
        }),
    });
    await api.install(page);

    await page.goto("/activity?tab=journal");
    await expect(page.getByRole("tab", { name: "Journal" })).toHaveCount(0);
    await expect(page.getByRole("tab", { name: "À traiter" })).toHaveAttribute("aria-selected", "true");
    expect(api.findRequest("GET", "/audit")).toBeUndefined();
});

test("les onglets fichiers, sauvegardes et métriques sont opérationnels et pilotés par capabilities", async ({ page }) => {
    const minecraftProfile = GAME_PROFILES.find((profile) => profile.id === "minecraft-java-vanilla")!;
    const operationalProfile = GameProfileSchema.parse({
        ...minecraftProfile,
        capabilities: ["settings", "files", "backups", "metrics"],
    });
    const api = new ApiMock({ profiles: [operationalProfile], instances: [INSTANCES[1]!] });
    await api.install(page);
    await page.goto(`/servers/${INSTANCES[1]!.id}`);

    await page.getByRole("tab", { name: "Fichiers" }).click();
    await expect(page.getByRole("button", { name: "server.properties", exact: true })).toBeVisible();
    await page.getByRole("button", { name: "Modifier server.properties" }).click();
    const editor = page.getByRole("textbox", { name: "Modifier server.properties" });
    await expect(editor).toContainText("motd=Serveur Dmx");
    await editor.fill("motd=Serveur E2E\nmax-players=10\n");
    await page.getByRole("button", { name: "Sauvegarder" }).click();
    expect(api.findRequest("PUT", "/files/text")?.headers["x-csrf-token"]).toBe("e2e-csrf-token");

    await page.getByLabel("Choisir un fichier à envoyer").setInputFiles({ name: "plugin.jar", mimeType: "application/java-archive", buffer: Buffer.from("jar") });
    await expect.poll(() => api.findRequest("PUT", "/files/content")).toBeTruthy();
    expect(api.findRequest("PUT", "/files/content")?.headers["content-type"]).toBe("application/octet-stream");

    await page.getByRole("tab", { name: "Sauvegardes" }).click();
    await expect(page.getByText("1 KB")).toBeVisible();
    await expect(page.getByRole("link", { name: "Télécharger" })).toHaveAttribute("href", /\/api\/v1\/backups\/.+\/download$/);
    await page.getByRole("button", { name: "Créer une sauvegarde" }).click();
    const createBackup = api.findRequest("POST", "/backups");
    expect(createBackup?.headers["idempotency-key"]).toBeTruthy();
    expect(createBackup?.headers["x-csrf-token"]).toBe("e2e-csrf-token");

    await page.getByRole("tab", { name: "Métriques" }).click();
    await expect(page.getByText("12.5 %", { exact: true }).first()).toBeVisible();
    await expect(page.getByText("512 MB", { exact: true }).first()).toBeVisible();
    expect(api.findRequest("GET", `/servers/${INSTANCES[1]!.id}/metrics`)?.search).toBe("?period=1d");
});

test("l’installation Modrinth envoie uniquement des identifiants typés et affiche l’artefact vérifié", async ({ page }) => {
    const minecraftProfile = GAME_PROFILES.find((candidate) => candidate.id === "minecraft-java-vanilla")!;
    const profile = GameProfileSchema.parse({
        ...minecraftProfile,
        id: "minecraft-java-paper",
        name: "Minecraft Java — Paper",
        capabilities: ["settings", "mods"],
    });
    const instance = {
        ...INSTANCES[1]!,
        profile_id: profile.id,
    };
    const api = new ApiMock({ profiles: [profile], instances: [instance] });
    await api.install(page);
    await page.goto(`/servers/${instance.id}`);
    await page.getByRole("tab", { name: "Mods" }).click();

    await page.getByLabel("Fournisseur").selectOption("modrinth");
    await page.getByLabel("ID du projet").fill("AABBCCDD");
    await page.getByLabel("ID de version/fichier").fill("IIJJKKLL");
    await page.getByRole("button", { name: "Installer la version" }).click();

    const request = api.findRequest("POST", `/servers/${instance.id}/mods/provider`);
    expect(request?.body).toEqual({
        provider: "modrinth",
        project_id: "AABBCCDD",
        version_id: "IIJJKKLL",
    });
    expect(request?.headers["x-csrf-token"]).toBe("e2e-csrf-token");
    await expect(page.getByText("provider-example.jar")).toBeVisible();
    await expect(page.getByRole("cell", { name: "Modrinth", exact: true })).toBeVisible();
});

test("les tâches planifiées restent limitées aux actions du profil et utilisent If-Match", async ({ page }) => {
    const valheimProfile = GAME_PROFILES.find((candidate) => candidate.id === "valheim")!;
    const profile = GameProfileSchema.parse({
        ...valheimProfile,
        capabilities: ["settings", "install", "lifecycle", "console", "backups"],
    });
    const api = new ApiMock({ profiles: [profile], instances: [INSTANCES[0]!] });
    await api.install(page);
    await page.goto(`/servers/${INSTANCES[0]!.id}`);

    await page.getByRole("tab", { name: "Tâches" }).click();
    await page.getByRole("button", { name: "Créer une tâche" }).click();
    await page.getByLabel("Nom").fill("Sauvegarde horaire");
    await page.getByLabel("Action fermée").selectOption("backup");
    await page.getByLabel("Intervalle en secondes").fill("3600");
    await page.getByRole("button", { name: "Sauvegarder" }).click();

    const create = api.findRequest("POST", "/schedules");
    expect(create?.body).toEqual({
        instance_id: INSTANCES[0]!.id,
        name: "Sauvegarde horaire",
        trigger: { kind: "interval", seconds: 3600 },
        action: { kind: "backup" },
        enabled: true,
    });
    expect(create?.headers["x-csrf-token"]).toBe("e2e-csrf-token");
    await expect(page.getByText("Sauvegarde horaire")).toBeVisible();
    await expect(page.getByText("Jamais")).toBeVisible();

    await page.getByRole("button", { name: "Modifier" }).click();
    await page.getByLabel("Déclencheur").selectOption("cron");
    await page.getByLabel("Expression cron").fill("0 0 3 * * *");
    await page.getByLabel("Fuseau IANA").fill("Europe/Paris");
    await page.getByRole("button", { name: "Sauvegarder" }).click();
    const update = api.findRequest("PUT", "/schedules/12121212-1212-4212-8212-121212121212");
    expect(update?.headers["if-match"]).toBe('"1"');
    expect(update?.body).toMatchObject({ trigger: { kind: "cron", expression: "0 0 3 * * *", timezone: "Europe/Paris" } });

    page.once("dialog", (dialog) => dialog.accept());
    await page.getByRole("button", { name: "Supprimer Sauvegarde horaire" }).click();
    await expect(page.getByText("Sauvegarde horaire")).toHaveCount(0);
});

test("l’autorisation appareil Hytale SSE reste éphémère et pointe uniquement vers le domaine officiel", async ({ page }) => {
    const api = new ApiMock({ hytaleDeviceAuthorization: true });
    await api.install(page);
    await page.goto(`/servers/${INSTANCES[0]!.id}`);

    await expect(page.getByRole("heading", { name: "Authentification Hytale requise" })).toBeVisible();
    await expect(page.getByText("x6nimECK", { exact: true })).toBeVisible();
    await expect(page.getByRole("link", { name: "Ouvrir Hytale" })).toHaveAttribute(
        "href",
        "https://oauth.accounts.hytale.com/oauth2/device/verify?user_code=x6nimECK",
    );
    const persisted = await page.evaluate(() => `${JSON.stringify(localStorage)}${JSON.stringify(sessionStorage)}`);
    expect(persisted).not.toContain("x6nimECK");
});
