import { expect, test } from "@playwright/test";
import { GameProfileSchema, InstanceSchema, JobSchema } from "../src/schemas/api";
import { ApiMock, GAME_PROFILES, INSTANCES, OWNER } from "./api.fixture";

const BEDROCK_INSTANCE_ID = "abababab-abab-4bab-8bab-abababababab";
const BEDROCK_JOB_ID = "cdcdcdcd-cdcd-4dcd-8dcd-cdcdcdcdcdcd";

const BEDROCK_PROFILE = GameProfileSchema.parse({
    id: "minecraft-bedrock",
    revision: 1,
    name: "Minecraft Bedrock",
    description: "Serveur Bedrock Dedicated Server officiel.",
    kind: "builtin",
    platforms: ["linux-x64", "windows-x64"],
    capabilities: ["settings", "install", "lifecycle", "console", "files", "backups", "metrics"],
    ports: [
        { name: "port", protocol: "udp", default: 19_132 },
        { name: "port_v6", protocol: "udp", default: 19_133 },
    ],
    lifecycle: { stop: { kind: "stdin", command: "stop", timeout_seconds: 60 }, ready_log_pattern: null },
    settings_schema: {
        type: "object",
        additionalProperties: false,
        required: ["version", "eula_accepted"],
        properties: {
            version: { type: "string", title: "Version", default: "1.21.0" },
            eula_accepted: { type: "boolean", title: "EULA acceptée", const: true, default: true },
        },
    },
    ui_schema: { layout: "sections" },
});

const BEDROCK_INSTANCE = InstanceSchema.parse({
    id: BEDROCK_INSTANCE_ID,
    name: "Bedrock famille",
    profile_id: "minecraft-bedrock",
    profile_revision: 1,
    settings: { version: "1.21.0", eula_accepted: true },
    config_version: 1,
    installation_state: "installing",
    installed_version: null,
    installed_build: null,
    desired_state: "stopped",
    runtime_state: "stopped",
    managed: true,
    auto_start: false,
    watchdog_enabled: true,
    created_at: "2026-07-13T12:00:00.000Z",
    updated_at: "2026-07-13T12:00:00.000Z",
});

const BEDROCK_WAITING_JOB = JobSchema.parse({
    id: BEDROCK_JOB_ID,
    instance_id: BEDROCK_INSTANCE_ID,
    kind: "install",
    state: "waiting_for_user",
    progress: 1,
    requested_by: OWNER.id,
    created_at: "2026-07-13T12:00:00.000Z",
    started_at: "2026-07-13T12:00:00.000Z",
    interaction: {
        kind: "bedrock_archive_upload",
        instance_id: BEDROCK_INSTANCE_ID,
        version: "1.21.0",
        method: "POST",
        path: `/api/v1/servers/${BEDROCK_INSTANCE_ID}/imports/zip`,
        required_sha256_header: "x-dmx-archive-sha256",
        max_bytes: 4 * 1024 * 1024 * 1024,
    },
});

test("Activité recharge les opérations persistantes et annule une installation", async ({ page }) => {
    const job = JobSchema.parse({
        id: "efefefef-efef-4fef-8fef-efefefefefef",
        instance_id: "22222222-2222-4222-8222-222222222222",
        kind: "install",
        state: "running",
        progress: 42,
        requested_by: OWNER.id,
        created_at: "2026-07-13T12:00:00.000Z",
        started_at: "2026-07-13T12:01:00.000Z",
        interaction: null,
    });
    const api = new ApiMock({ jobs: [job] });
    await api.install(page);

    await page.goto(`/activity?tab=operations&focus=${job.id}&instance=${job.instance_id}`);
    const drawer = page.getByRole("dialog");
    await expect(drawer.getByRole("heading", { name: "Installation / mise à jour" })).toBeVisible();
    await expect(drawer.getByText("42%", { exact: true })).toBeVisible();
    await page.getByRole("button", { name: "Annuler le job" }).click();
    await page.getByRole("dialog", { name: "Annuler le job" }).getByRole("button", { name: "Annuler le job" }).click();

    await expect(drawer.getByText("Annulé", { exact: true })).toBeVisible();
    expect(api.findRequest("POST", `/jobs/${job.id}/cancel`)?.headers["x-csrf-token"]).toBe("e2e-csrf-token");
});

test("Activité affiche l’action humaine persistante et donne accès au terminal d’installation", async ({ page }) => {
    const job = JobSchema.parse({
        id: "dededede-dede-4ded-8ded-dededededede",
        instance_id: "22222222-2222-4222-8222-222222222222",
        kind: "install",
        state: "waiting_for_user",
        progress: 12,
        requested_by: OWNER.id,
        created_at: "2026-07-13T12:00:00.000Z",
        started_at: "2026-07-13T12:01:00.000Z",
        interaction: {
            kind: "oauth_device",
            verification_uri: "https://oauth.accounts.hytale.com/oauth2/device/verify?user_code=x6nimECK",
            user_code: "x6nimECK",
        },
    });
    const api = new ApiMock({ jobs: [job] });
    await api.install(page);

    await page.goto(`/activity?tab=attention&focus=${job.id}`);

    await expect(page.getByRole("heading", { name: "Authentification Hytale requise" })).toBeVisible();
    await expect(page.getByText("x6nimECK", { exact: true })).toBeVisible();
    await expect(page.getByRole("link", { name: "Ouvrir Hytale" })).toHaveAttribute("href", "https://oauth.accounts.hytale.com/oauth2/device/verify?user_code=x6nimECK");
    await expect(page.getByRole("link", { name: "Voir le terminal d’installation" })).toHaveAttribute(
        "href",
        "/servers/22222222-2222-4222-8222-222222222222?tab=console&source=install&job=dededede-dede-4ded-8ded-dededededede",
    );
});

test("la mise à jour manuelle affiche la version installée et crée un job", async ({ page }) => {
    const minecraft = GAME_PROFILES.find((profile) => profile.id === "minecraft-java-vanilla")!;
    const updateProfile = GameProfileSchema.parse({
        ...minecraft,
        capabilities: ["settings", "install", "lifecycle", "console", "files", "backups", "metrics"],
    });
    const api = new ApiMock({
        profiles: GAME_PROFILES.map((profile) => profile.id === updateProfile.id ? updateProfile : profile),
        updateAvailable: true,
    });
    await api.install(page);

    await page.goto("/servers/22222222-2222-4222-8222-222222222222");
    // Ciblé sur la vignette : l'avis de mise à jour affiche lui aussi la version
    // installée, en tête de l'écart avec la version disponible.
    const installedPill = page.locator(".stat-pill").filter({ hasText: "Version installée" });
    await expect(installedPill).toBeVisible();
    await expect(installedPill.locator(".stat-pill__value")).toHaveText("1.21.8");
    // Le terminal est désormais l'onglet par défaut ; les diagnostics vivent
    // dans la configuration.
    await page.getByRole("tab", { name: "Config" }).click();
    await page.getByText("Diagnostics internes").click();
    await expect(page.getByText("server.jar", { exact: true })).toBeVisible();
    await page.getByRole("button", { name: "Mettre à jour le jeu" }).click();

    await expect(page).toHaveURL(/\/servers\/22222222-2222-4222-8222-222222222222\?tab=console&job=44444444-4444-4444-8444-444444444444/);
    expect(api.findRequest("POST", "/servers/22222222-2222-4222-8222-222222222222/actions/install")).toBeDefined();
});

test("le bouton de mise à jour reste absent quand le jeu est à jour", async ({ page }) => {
    // Sur un profil dont le fournisseur impose la version — ici Valheim. Le cas
    // Minecraft est différent et couvert par le test suivant : sa version étant
    // choisie, l'action reste offerte alors qu'aucun avis n'est rendu.
    const valheim = GAME_PROFILES.find((profile) => profile.id === "valheim")!;
    const profile = GameProfileSchema.parse({
        ...valheim,
        capabilities: ["settings", "install", "lifecycle", "console", "files", "backups", "metrics"],
    });
    const api = new ApiMock({
        profiles: GAME_PROFILES.map((candidate) => candidate.id === profile.id ? profile : candidate),
        updateAvailable: false,
    });
    await api.install(page);

    await page.goto(`/servers/${INSTANCES[0]!.id}`);

    await expect.poll(() => api.findRequest(
        "GET",
        `/servers/${INSTANCES[0]!.id}/update-status`,
    )).toBeDefined();
    await expect(page.getByText("Jeu à jour d’après le fournisseur.")).toBeVisible();
    await expect(page.getByRole("button", { name: "Mettre à jour le jeu" })).toHaveCount(0);
});

test("un profil Minecraft n’annonce aucune mise à jour mais garde l’action", async ({ page }) => {
    // La version d'un serveur Minecraft est choisie délibérément, pour la
    // compatibilité des mods : l'annoncer en retard désignait comme un retard ce
    // qui est une décision. L'action, elle, doit rester joignable — c'est elle
    // qui applique la version retenue en configuration.
    const minecraft = GAME_PROFILES.find((profile) => profile.id === "minecraft-java-vanilla")!;
    const profile = GameProfileSchema.parse({
        ...minecraft,
        capabilities: ["settings", "install", "lifecycle", "console", "files", "backups", "metrics"],
    });
    const api = new ApiMock({
        profiles: GAME_PROFILES.map((candidate) => candidate.id === profile.id ? profile : candidate),
        updateAvailable: true,
    });
    await api.install(page);

    await page.goto("/servers/22222222-2222-4222-8222-222222222222");

    await expect.poll(() => api.findRequest(
        "GET",
        "/servers/22222222-2222-4222-8222-222222222222/update-status",
    )).toBeDefined();
    await expect(page.locator(".update-notice")).toHaveCount(0);
    await expect(page.getByText("Mise à jour du jeu disponible")).toHaveCount(0);
    await expect(page.getByText("Jeu à jour d’après le fournisseur.")).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Mettre à jour le jeu" })).toBeVisible();
});

test("un refresh restaure l’interaction Bedrock persistée puis reprend le même job avec SHA-256", async ({ page }) => {
    const api = new ApiMock({
        profiles: [...GAME_PROFILES, BEDROCK_PROFILE],
        instances: [BEDROCK_INSTANCE],
        jobs: [BEDROCK_WAITING_JOB],
    });
    await api.install(page);

    await page.goto(`/servers/${BEDROCK_INSTANCE_ID}`);
    await expect(page.getByRole("heading", { name: "Archive Minecraft Bedrock requise" })).toBeVisible();
    await expect(page.getByText("Version attendue :")).toBeVisible();
    await page.getByLabel("Archive ZIP officielle Minecraft Bedrock").setInputFiles({
        name: "bedrock-server.zip",
        mimeType: "application/zip",
        buffer: Buffer.from([0x50, 0x4b, 0x03, 0x04, 0x00, 0x00]),
    });
    const digest = "a".repeat(64);
    await page.getByLabel("SHA-256 publié ou vérifié de l’archive").fill(digest);
    await page.getByRole("button", { name: "Envoyer et reprendre" }).click();

    await expect(page.getByRole("heading", { name: "Archive Minecraft Bedrock requise" })).toHaveCount(0);
    const request = api.findRequest("POST", `/servers/${BEDROCK_INSTANCE_ID}/imports/zip`);
    expect(request?.headers["x-dmx-archive-sha256"]).toBe(digest);
    expect(request?.headers["idempotency-key"]).toBe(BEDROCK_JOB_ID);
    expect(api.jobs[0]?.id).toBe(BEDROCK_JOB_ID);
    expect(api.jobs[0]?.state).toBe("queued");
});

test("une instance en marche signale quand même une mise à jour disponible", async ({ page }) => {
    // Le bouton de mise à jour était masqué tant que l'instance tournait : rien
    // n'indiquait alors qu'une version plus récente existait, et il fallait
    // interroger le jeu lui-même pour l'apprendre.
    const api = new ApiMock({ updateAvailable: true });
    await api.install(page);

    await page.goto(`/servers/${INSTANCES[0]!.id}`);

    await expect(page.getByText("Mise à jour du jeu disponible")).toBeVisible();
    // Les deux références sont désormais confrontées côte à côte plutôt que
    // noyées dans une phrase à trous.
    const versions = page.locator(".update-notice__versions");
    await expect(versions).toContainText("0.219.16");
    await expect(versions.locator(".update-notice__version--next")).not.toBeEmpty();
    await expect(page.getByText("Arrêtez complètement l’instance pour appliquer la mise à jour.")).toBeVisible();
});

test("aucun avis de mise à jour quand le jeu est à jour", async ({ page }) => {
    const api = new ApiMock({ updateAvailable: false });
    await api.install(page);

    await page.goto(`/servers/${INSTANCES[0]!.id}`);

    await expect.poll(() => api.findRequest("GET", `/servers/${INSTANCES[0]!.id}/update-status`)).toBeDefined();
    await expect(page.getByText("Mise à jour du jeu disponible")).toHaveCount(0);
});

test("un échec de vérification est signalé au lieu de rester invisible", async ({ page }) => {
    // Auparavant seul `update_available` produisait un affichage : « à jour » et
    // « vérification impossible » étaient tous deux rendus par un écran vide,
    // donc indiscernables.
    const api = new ApiMock();
    await api.install(page);
    await page.route(`**/api/v1/servers/${INSTANCES[0]!.id}/update-status*`, (route) => route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
            state: "check_failed",
            installed_version: "0.219.16",
            installed_build: null,
            available_version: null,
            available_build: null,
            checked_at: "2026-07-13T12:00:00.000Z",
        }),
    }));

    await page.goto(`/servers/${INSTANCES[0]!.id}`);
    await expect(page.getByText("Impossible de vérifier les mises à jour")).toBeVisible();
    await expect(page.getByRole("button", { name: "Vérifier maintenant" })).toBeVisible();
});

test("« à jour » reste visible et rejouable", async ({ page }) => {
    const api = new ApiMock({ updateAvailable: false });
    await api.install(page);

    await page.goto(`/servers/${INSTANCES[0]!.id}`);
    await expect(page.getByText("Jeu à jour d’après le fournisseur.")).toBeVisible();
    await page.getByRole("button", { name: "Vérifier maintenant" }).click();
    await expect.poll(() => api.requests.some((request) =>
        request.method === "GET"
        && request.path === `/servers/${INSTANCES[0]!.id}/update-status`
        && request.search?.includes("refresh=true"))).toBe(true);
});

test("la mise à jour se voit depuis la liste et le tableau de bord", async ({ page }) => {
    // Il fallait auparavant ouvrir chaque instance une par une pour savoir
    // laquelle était en retard.
    const api = new ApiMock({ updateAvailable: true });
    await api.install(page);

    await page.goto("/servers");
    await expect(page.getByText("Mise à jour", { exact: true }).first()).toBeVisible();

    // Les profils Minecraft n'en portent jamais : leur version est arrêtée par
    // l'utilisateur pour garder les mods compatibles. Les y annoncer en retard
    // noyait les instances qui attendaient réellement une mise à jour.
    const rows = page.locator(".server-list-table tbody tr, .server-card");
    const minecraftRow = rows.filter({ hasText: "Minecraft" });
    await expect(minecraftRow).not.toHaveCount(0);
    await expect(minecraftRow.locator(".update-pill")).toHaveCount(0);

    await page.goto("/dashboard");
    const counter = page.getByRole("region", { name: "Vue d’ensemble opérationnelle" })
        .or(page.locator(".dashboard-header-stats"));
    await expect(counter.getByText("Mises à jour disponibles")).toBeVisible();

    // Le décompte vient d'un seul appel servi depuis les verdicts conservés,
    // sans déclencher une vérification par instance.
    await expect.poll(() => api.requests.filter((request) =>
        request.path === "/servers/update-status").length).toBeGreaterThan(0);
    expect(api.requests.filter((request) => /\/servers\/[0-9a-f-]+\/update-status/.test(request.path))).toHaveLength(0);
});
