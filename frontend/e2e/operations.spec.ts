import { expect, test } from "@playwright/test";
import { GameProfileSchema } from "../src/schemas/api";
import { ApiMock, GAME_PROFILES, INSTANCES } from "./api.fixture";

test("le chat est paginé, persistant, temps réel et protégé par CSRF", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);

    await page.goto("/chat");
    await expect(page.getByRole("heading", { name: "Chat d’équipe" })).toBeVisible();
    await expect(page.getByText("Bienvenue dans le chat d’équipe.")).toBeVisible();
    await expect(page.getByText("Message reçu en temps réel.")).toBeVisible();

    await page.getByLabel("Nouveau message").fill("Message depuis Playwright");
    await page.getByRole("button", { name: "Envoyer" }).click();
    await expect(page.getByText("Message depuis Playwright")).toBeVisible();

    const request = api.findRequest("POST", "/chat");
    expect(request?.body).toEqual({ body: "Message depuis Playwright" });
    expect(request?.headers["x-csrf-token"]).toBe("e2e-csrf-token");
    expect(request?.headers.authorization).toBeUndefined();
    const events = api.findRequest("GET", "/events");
    expect(events?.search).toBe("");
    expect(events?.headers.authorization).toBeUndefined();
});

test("le centre de notifications applique le ciblage, le filtre non lu et le temps réel", async ({ page }) => {
    const api = new ApiMock();
    await api.install(page);

    await page.goto("/notifications");
    await expect(page.getByRole("heading", { name: "Centre de notifications" })).toBeVisible();
    await expect(page.getByText("L’opération s’est terminée avec succès.")).toBeVisible();
    await expect(page.getByText("L’opération a échoué.")).toBeVisible();

    await page.getByRole("button", { name: "Tout marquer comme lu" }).click();
    await expect(page.getByText(/^0 non lue/)).toBeVisible();
    const readAll = api.findRequest("POST", "/notifications/read-all");
    expect(readAll?.headers["x-csrf-token"]).toBe("e2e-csrf-token");

    await page.getByLabel("Non lues uniquement").check();
    await expect(page.getByText("Aucune notification à afficher.")).toBeVisible();
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
    await expect(page.getByText("ABCD-1234", { exact: true })).toBeVisible();
    await expect(page.getByRole("link", { name: "Ouvrir Hytale" })).toHaveAttribute(
        "href",
        "https://accounts.hytale.com/device?user_code=ABCD-1234",
    );
    const persisted = await page.evaluate(() => `${JSON.stringify(localStorage)}${JSON.stringify(sessionStorage)}`);
    expect(persisted).not.toContain("ABCD-1234");
});
