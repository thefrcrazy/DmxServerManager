import { Page, Request, Route } from "@playwright/test";
import {
    AuthResponseSchema,
    GameProfile,
    GameProfileSchema,
    Instance,
    InstanceGrant,
    InstanceGrantSchema,
    InstanceSchema,
    Job,
    JobSchema,
    ManagedRole,
    ManagedRoleSchema,
    ManagedUser,
    ManagedUserSchema,
    PermissionDescription,
    PermissionDescriptionSchema,
    UserInfo,
    UserInfoSchema,
} from "../src/schemas/api";
import {
    Backup,
    BackupSchema,
    ManagedFileEntry,
    ManagedFileEntrySchema,
    MetricPoint,
    MetricPointSchema,
    Schedule,
    ScheduleSchema,
    CreateScheduleSchema,
    UpdateScheduleSchema,
    SteamProfileDefinition,
    SteamProfileDefinitionSchema,
    CreateSteamProfileSchema,
    CreateDiscordWebhookSchema,
    DiscordWebhook,
    DiscordWebhookSchema,
    InstalledModSchema,
    UpdateDiscordWebhookSchema,
} from "../src/schemas/operations";
import { PERMISSION_CATALOG } from "../src/constants/permissions";
import { DEFAULT_THEME_TOKENS } from "../src/constants/theme";
import { PanelReleaseStatus, PanelReleaseStatusSchema } from "../src/schemas/releases";
import {
    ActiveTheme,
    ActiveThemeSchema,
    CatalogPackage,
    CatalogPackageSchema,
    ThemeSelectionSchema,
} from "../src/schemas/catalog";

const API_PREFIX = "/api/v1";
const SESSION_COOKIE = "dmx_session";
const SESSION_TOKEN = "e2e-opaque-session-token";
const CSRF_TOKEN = "e2e-csrf-token";
const NOW = "2026-07-13T12:00:00.000Z";

function customSteamProfile(id: string, revision: number, definition: SteamProfileDefinition): GameProfile {
    const platforms = [
        ...(definition.executable.linux_x86_64 ? ["linux-x64" as const] : []),
        ...(definition.executable.windows_x86_64 ? ["windows-x64" as const] : []),
    ];
    return GameProfileSchema.parse({
        id,
        revision,
        name: definition.name,
        description: definition.description,
        kind: "steam_custom",
        platforms,
        capabilities: ["settings"],
        ports: definition.ports,
        lifecycle: { stop: definition.stop_strategy, ready_log_pattern: definition.ready_log_pattern },
        settings_schema: {
            type: "object",
            additionalProperties: false,
            required: [],
            properties: Object.fromEntries(definition.ports.map((port) => [port.name, {
                type: "integer", minimum: 1, maximum: 65_535, default: port.default,
            }])),
        },
        ui_schema: { layout: "sections" },
        steam_profile: {
            app_id: definition.app_id,
            branch: definition.branch,
            executable: definition.executable,
            arguments: definition.arguments,
            ports: definition.ports,
            save_paths: definition.save_paths,
            ready_log_pattern: definition.ready_log_pattern,
            stop_strategy: definition.stop_strategy,
        },
    });
}

export const OWNER: UserInfo = UserInfoSchema.parse({
    id: "019f5c30-6557-7583-8d27-03a9cc043572",
    username: "owner",
    role: "owner",
    permissions: ["*"],
    language: "fr",
    accent_color: "#4f46e5",
    must_change_password: false,
});

export const GAME_PROFILES: GameProfile[] = [
    GameProfileSchema.parse({
        id: "minecraft-java",
        revision: 1,
        name: "Minecraft Java",
        description: "Serveur Minecraft Java avec loader, version et runtime Java configurables.",
        kind: "builtin",
        platforms: ["linux-x64", "windows-x64"],
        capabilities: ["settings", "install", "lifecycle", "console", "files", "backups", "metrics", "mods"],
        ports: [{ name: "port", protocol: "tcp", default: 25565 }],
        lifecycle: {
            stop: { kind: "stdin", command: "stop", timeout_seconds: 60 },
            ready_log_pattern: "Done \\(.+\\)! For help",
        },
        settings_schema: {
            type: "object",
            additionalProperties: false,
            required: ["loader", "version", "eula_accepted"],
            properties: {
                loader: {
                    type: "string",
                    title: "Loader",
                    enum: ["vanilla", "paper", "fabric", "forge", "neoforge", "spigot", "purpur", "quilt"],
                    default: "vanilla",
                },
                version: { type: "string", title: "Version", minLength: 1, maxLength: 64, default: "1.21.8" },
                loader_version: {
                    type: "string",
                    title: "Version du loader",
                    minLength: 1,
                    maxLength: 96,
                    pattern: "^[A-Za-z0-9._+-]+$",
                },
                port: { type: "integer", title: "Port TCP", minimum: 1, maximum: 65_535, default: 25_565 },
                max_memory_mb: { type: "integer", title: "Mémoire maximale", minimum: 512, maximum: 131_072, default: 4096 },
                eula_accepted: { type: "boolean", title: "J’accepte le contrat EULA", const: true, default: false },
            },
        },
        ui_schema: { layout: "sections" },
    }),
    GameProfileSchema.parse({
        id: "minecraft-java-vanilla",
        revision: 1,
        name: "Minecraft Java — Vanilla",
        description: "Serveur Minecraft Java avec version et runtime Java épinglés.",
        kind: "builtin",
        platforms: ["linux-x64", "windows-x64"],
        capabilities: ["settings"],
        ports: [{ name: "port", protocol: "tcp", default: 25565 }],
        lifecycle: {
            stop: { kind: "stdin", command: "stop", timeout_seconds: 60 },
            ready_log_pattern: "Done \\(.+\\)! For help",
        },
        settings_schema: {
            type: "object",
            additionalProperties: false,
            required: ["version", "eula_accepted"],
            properties: {
                version: { type: "string", title: "Version", minLength: 1, maxLength: 64, default: "1.21.8" },
                port: { type: "integer", title: "Port TCP", minimum: 1, maximum: 65_535, default: 25_565 },
                eula_accepted: { type: "boolean", title: "J’accepte le contrat EULA", const: true, default: false },
            },
        },
        ui_schema: { layout: "sections" },
    }),
    GameProfileSchema.parse({
        id: "valheim",
        revision: 1,
        name: "Valheim",
        description: "Serveur Valheim installé anonymement par SteamCMD (AppID 896660).",
        kind: "builtin",
        platforms: ["linux-x64", "windows-x64"],
        capabilities: ["settings", "secrets", "install", "lifecycle", "console"],
        ports: [
            { name: "port", protocol: "udp", default: 2456 },
            { name: "query_port", protocol: "udp", default: 2457, adjacent_to: "port" },
        ],
        lifecycle: {
            stop: { kind: "interrupt", timeout_seconds: 60 },
            ready_log_pattern: "Game server connected",
        },
        settings_schema: {
            type: "object",
            additionalProperties: false,
            required: ["server_name", "world_name", "server_password"],
            properties: {
                server_name: { type: "string", title: "Nom public", minLength: 1, maxLength: 64, default: "Valheim Dmx" },
                world_name: { type: "string", title: "Monde", minLength: 1, maxLength: 64, default: "Dedicated" },
                port: { type: "integer", title: "Port UDP", minimum: 1, maximum: 65_534, default: 2456 },
                query_port: { type: "integer", title: "Port requête UDP", minimum: 2, maximum: 65_535, default: 2457 },
                crossplay: { type: "boolean", title: "Crossplay", default: false },
                server_password: {
                    type: "string",
                    title: "Mot de passe serveur",
                    minLength: 5,
                    maxLength: 64,
                    secret: true,
                    writeOnly: true,
                },
            },
        },
        ui_schema: { layout: "sections" },
    }),
    GameProfileSchema.parse({
        id: "steam-example",
        revision: 1,
        name: "Steam Example",
        description: "Profil SteamCMD anonyme global et versionné.",
        kind: "steam_custom",
        platforms: ["linux-x64"],
        capabilities: ["settings", "install", "lifecycle", "console", "files", "backups", "metrics"],
        ports: [{ name: "game", protocol: "udp", default: 27_015 }],
        lifecycle: { stop: { kind: "terminate", timeout_seconds: 30 }, ready_log_pattern: null },
        settings_schema: {
            type: "object",
            additionalProperties: false,
            required: [],
            properties: {
                game: { type: "integer", title: "Port UDP", minimum: 1, maximum: 65_535, default: 27_015 },
            },
        },
        ui_schema: { layout: "sections" },
        steam_profile: {
            app_id: 123_456,
            branch: null,
            executable: { linux_x86_64: "bin/server", windows_x86_64: null },
            arguments: ["--port", "{{port:game}}"],
            ports: [{ name: "game", protocol: "udp", default: 27_015 }],
            save_paths: ["saves"],
            ready_log_pattern: null,
            stop_strategy: { kind: "terminate", timeout_seconds: 30 },
        },
    }),
];

export const INSTANCES: Instance[] = [
    InstanceSchema.parse({
        id: "11111111-1111-4111-8111-111111111111",
        name: "Survie Valheim",
        profile_id: "valheim",
        profile_revision: 1,
        settings: {
            server_name: "Survie Valheim",
            world_name: "Dedicated",
            port: 2456,
            query_port: 2457,
            crossplay: true,
        },
        config_version: 3,
        installation_state: "installed",
        installed_version: "0.219.16",
        installed_build: "steam-896660-20260713",
        desired_state: "running",
        runtime_state: "running",
        managed: true,
        auto_start: true,
        watchdog_enabled: true,
        created_at: NOW,
        updated_at: NOW,
    }),
    InstanceSchema.parse({
        id: "22222222-2222-4222-8222-222222222222",
        name: "Minecraft sans driver runtime",
        profile_id: "minecraft-java-vanilla",
        profile_revision: 1,
        settings: { version: "1.21.8", port: 25_565, eula_accepted: true },
        config_version: 1,
        installation_state: "installed",
        installed_version: "1.21.8",
        installed_build: "server.jar",
        desired_state: "stopped",
        runtime_state: "stopped",
        managed: true,
        auto_start: false,
        watchdog_enabled: false,
        created_at: NOW,
        updated_at: NOW,
    }),
];

export const ROLES: ManagedRole[] = [
    ManagedRoleSchema.parse({
        id: "owner",
        name: "Owner",
        permissions: ["*"],
        is_system: true,
        created_at: NOW,
        updated_at: NOW,
    }),
    ManagedRoleSchema.parse({
        id: "admin",
        name: "Admin",
        permissions: [
            "audit.read", "job.read", "mods.manage", "panel.network.manage", "profile.read",
            "schedule.manage", "server.backup", "server.backup.read", "server.config.raw.read", "server.config.raw.write", "server.console.read", "server.console.write",
            "server.create", "server.delete", "server.files.read", "server.files.write", "server.kill", "server.read",
            "server.start", "server.stop", "server.update", "server.update_game", "user.create", "user.read", "user.update",
        ],
        is_system: true,
        created_at: NOW,
        updated_at: NOW,
    }),
    ManagedRoleSchema.parse({
        id: "operator",
        name: "Operator",
        permissions: [
            "job.read", "mods.manage", "profile.read",
            "schedule.manage", "server.backup", "server.backup.read", "server.console.read", "server.console.write",
            "server.files.read", "server.files.write", "server.read", "server.start", "server.stop", "server.update",
            "server.update_game",
        ],
        is_system: true,
        created_at: NOW,
        updated_at: NOW,
    }),
    ManagedRoleSchema.parse({
        id: "viewer",
        name: "Viewer",
        permissions: ["job.read", "profile.read", "server.backup.read", "server.console.read", "server.read"],
        is_system: true,
        created_at: NOW,
        updated_at: NOW,
    }),
];

export const USERS: ManagedUser[] = [
    ManagedUserSchema.parse({
        id: OWNER.id,
        username: OWNER.username,
        role_id: "owner",
        role_name: "Owner",
        is_active: true,
        language: "fr",
        accent_color: "#4f46e5",
        must_change_password: false,
        last_login_at: NOW,
        created_at: NOW,
        updated_at: NOW,
    }),
    ManagedUserSchema.parse({
        id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        username: "alice",
        role_id: "operator",
        role_name: "Operator",
        is_active: true,
        language: "fr",
        accent_color: "#3a82f6",
        must_change_password: false,
        last_login_at: null,
        created_at: NOW,
        updated_at: NOW,
    }),
];

export const PERMISSIONS: PermissionDescription[] = PERMISSION_CATALOG.map((permission) => (
    PermissionDescriptionSchema.parse(permission)
));

const CATALOG_THEME_TOKENS = {
    ...DEFAULT_THEME_TOKENS,
    bg_primary: "#080B14",
    bg_secondary: "#0D1321",
    bg_tertiary: "#131B2E",
    bg_elevated: "#19243A",
    border: "#334155",
    border_hover: "#94A3B8",
} as const;

export const CATALOG_PACKAGES: CatalogPackage[] = [CatalogPackageSchema.parse({
    id: "theme-midnight",
    revision: 1,
    kind: "theme",
    schema_version: 1,
    name: "Midnight",
    description: "Thème sombre local et accessible.",
    archive_sha256: "b".repeat(64),
    archive_size_bytes: 1_024,
    content_size_bytes: 512,
    manifest: {
        format: "dmxpack",
        schema_version: 1,
        id: "theme-midnight",
        revision: 1,
        name: "Midnight",
        description: "Thème sombre local et accessible.",
        content: { kind: "theme", tokens: "tokens.json", logo: null, preview: null },
        files: [{
            path: "tokens.json",
            sha256: "c".repeat(64),
            size_bytes: 512,
            media_type: "application/json",
        }],
    },
    files: [{
        role: "tokens",
        path: "tokens.json",
        media_type: "application/json",
        sha256: "c".repeat(64),
        size_bytes: 512,
    }],
    theme_tokens: CATALOG_THEME_TOKENS,
    compatibility_status: "unverified",
    created_at: NOW,
})];

const DEFAULT_ACTIVE_THEME: ActiveTheme = ActiveThemeSchema.parse({
    selection: { kind: "default" },
    tokens: DEFAULT_THEME_TOKENS,
    assets: { logo: null, preview: null },
    version: 1,
    updated_at: NOW,
});

export const GRANTS: Record<string, InstanceGrant[]> = {
    "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa": [InstanceGrantSchema.parse({
        instance_id: INSTANCES[0]!.id,
        instance_name: INSTANCES[0]!.name,
        permissions: [],
        created_at: NOW,
    })],
};

const FILES: ManagedFileEntry[] = [
    ManagedFileEntrySchema.parse({ name: "server.properties", path: "server.properties", kind: "file", size_bytes: 32, modified_at: NOW }),
    ManagedFileEntrySchema.parse({ name: "world", path: "world", kind: "directory", size_bytes: 0, modified_at: NOW }),
];

const BACKUPS: Backup[] = [BackupSchema.parse({
    id: "77777777-7777-4777-8777-777777777777",
    instance_id: INSTANCES[1]!.id,
    kind: "manual",
    status: "ready",
    checksum_sha256: "a".repeat(64),
    size_bytes: 1_024,
    created_at: NOW,
    completed_at: NOW,
})];

const METRICS: MetricPoint[] = [MetricPointSchema.parse({
    id: "88888888-8888-4888-8888-888888888888",
    cpu_usage: 12.5,
    memory_bytes: 536_870_912,
    disk_bytes: 1_073_741_824,
    uptime_seconds: 3_661,
    player_count: 1,
    recorded_at: NOW,
})];

export interface RecordedApiRequest {
    method: string;
    path: string;
    search: string;
    headers: Record<string, string>;
    body: unknown;
}

export interface ApiMockOptions {
    authenticated?: boolean;
    needsSetup?: boolean;
    user?: UserInfo;
    profiles?: GameProfile[];
    instances?: Instance[];
    roles?: ManagedRole[];
    users?: ManagedUser[];
    permissions?: PermissionDescription[];
    grants?: Record<string, InstanceGrant[]>;
    webhooks?: DiscordWebhook[];
    releaseStatus?: PanelReleaseStatus;
    hytaleDeviceAuthorization?: boolean;
    jobs?: Job[];
    catalogPackages?: CatalogPackage[];
    activeTheme?: ActiveTheme;
    updateAvailable?: boolean;
}

function requestBody(request: Request): unknown {
    const raw = request.postData();
    if (!raw) return null;
    try {
        return JSON.parse(raw) as unknown;
    } catch {
        return raw;
    }
}

function matchesProvider(value: unknown): value is "modrinth" | "curseforge" {
    return value === "modrinth" || value === "curseforge";
}

export class ApiMock {
    readonly requests: RecordedApiRequest[] = [];
    readonly profiles: GameProfile[];
    readonly instances: Instance[];
    readonly roles: ManagedRole[];
    readonly users: ManagedUser[];
    readonly permissions: PermissionDescription[];
    readonly profileRevisions = new Map<string, GameProfile[]>();
    readonly grants: Map<string, InstanceGrant[]>;
    readonly user: UserInfo;
    readonly files = FILES.map((item) => ManagedFileEntrySchema.parse(item));
    readonly backups = BACKUPS.map((item) => BackupSchema.parse(item));
    readonly metrics = METRICS.map((item) => MetricPointSchema.parse(item));
    readonly jobs: Job[];
    readonly schedules: Schedule[] = [];
    readonly webhooks: DiscordWebhook[];
    readonly catalogPackages: CatalogPackage[];
    activeTheme: ActiveTheme;
    releaseStatus: PanelReleaseStatus;
    curseForgeConfigured = false;
    advertisedGameHost: string | null = "play.example.com";
    networkVersion = 1;
    needsSetup: boolean;
    authenticated: boolean;
    readonly hytaleDeviceAuthorization: boolean;
    readonly updateAvailable: boolean;

    constructor(options: ApiMockOptions = {}) {
        this.authenticated = options.authenticated ?? true;
        this.needsSetup = options.needsSetup ?? false;
        this.hytaleDeviceAuthorization = options.hytaleDeviceAuthorization ?? false;
        this.updateAvailable = options.updateAvailable ?? false;
        this.user = UserInfoSchema.parse(options.user ?? OWNER);
        this.profiles = (options.profiles ?? GAME_PROFILES).map((profile) => GameProfileSchema.parse(profile));
        for (const profile of this.profiles) {
            if (profile.kind === "steam_custom") this.profileRevisions.set(profile.id, [profile]);
        }
        this.instances = (options.instances ?? INSTANCES).map((instance) => InstanceSchema.parse(instance));
        this.jobs = (options.jobs ?? []).map((job) => JobSchema.parse(job));
        this.roles = (options.roles ?? ROLES).map((role) => ManagedRoleSchema.parse(role));
        this.users = (options.users ?? USERS).map((user) => ManagedUserSchema.parse(user));
        this.permissions = (options.permissions ?? PERMISSIONS).map((permission) => PermissionDescriptionSchema.parse(permission));
        this.grants = new Map(Object.entries(options.grants ?? GRANTS).map(([userId, grants]) => [
            userId,
            grants.map((grant) => InstanceGrantSchema.parse(grant)),
        ]));
        this.webhooks = (options.webhooks ?? []).map((webhook) => DiscordWebhookSchema.parse(webhook));
        this.catalogPackages = (options.catalogPackages ?? CATALOG_PACKAGES)
            .map((catalogPackage) => CatalogPackageSchema.parse(catalogPackage));
        this.activeTheme = ActiveThemeSchema.parse(options.activeTheme ?? DEFAULT_ACTIVE_THEME);
        this.releaseStatus = PanelReleaseStatusSchema.parse(options.releaseStatus ?? {
            configured: true,
            current_version: "1.0.0",
            deployment_mode: "native",
            state: "update_available",
            checked_at: NOW,
            latest: {
                version: "1.0.1",
                published_at: NOW,
                notes_url: "https://github.com/thefrcrazy/DmxServerManager/releases/tag/v1.0.1",
                target: {
                    kind: "native",
                    platform: "linux-amd64",
                    archive_url: "https://github.com/thefrcrazy/DmxServerManager/releases/download/v1.0.1/dmx-server-manager-linux-amd64.tar.gz",
                    archive_sha256: "a".repeat(64),
                    installer_url: "https://github.com/thefrcrazy/DmxServerManager/releases/download/v1.0.1/dmx-server-manager-install-linux.sh",
                    installer_sha256: "b".repeat(64),
                    upgrade_command: `p=$(mktemp /tmp/dmx-server-manager-install.XXXXXX) && trap 'rm -f "$p"' EXIT HUP INT TERM && curl --fail --location --proto '=https' --proto-redir '=https' --tlsv1.2 --output "$p" 'https://github.com/thefrcrazy/DmxServerManager/releases/download/v1.0.1/dmx-server-manager-install-linux.sh' && printf '%s  %s\\n' '${"b".repeat(64)}' "$p" | sha256sum --check --status && sudo DMX_VERSION='1.0.1' DMX_EXPECTED_ARCHIVE_SHA256='${"a".repeat(64)}' sh "$p"`,
                },
            },
            error_code: null,
        });
    }

    async install(page: Page): Promise<void> {
        if (this.authenticated) await this.addSessionCookie(page);
        await page.route("**/api/v1/**", (route) => this.handle(route, page));
    }

    findRequest(method: string, path: string): RecordedApiRequest | undefined {
        for (let index = this.requests.length - 1; index >= 0; index -= 1) {
            const request = this.requests[index];
            if (request?.method === method && request.path === path) return request;
        }
        return undefined;
    }

    private async addSessionCookie(page: Page): Promise<void> {
        await page.context().addCookies([{
            name: SESSION_COOKIE,
            value: SESSION_TOKEN,
            domain: "127.0.0.1",
            path: API_PREFIX,
            httpOnly: true,
            secure: false,
            sameSite: "Strict",
        }]);
    }

    private async handle(route: Route, page: Page): Promise<void> {
        const request = route.request();
        const url = new URL(request.url());
        const path = url.pathname.slice(API_PREFIX.length) || "/";
        const record: RecordedApiRequest = {
            method: request.method(),
            path,
            search: url.search,
            headers: await request.allHeaders(),
            body: requestBody(request),
        };
        this.requests.push(record);

        if (path === "/auth/status" && request.method() === "GET") {
            return this.json(route, 200, { needs_setup: this.needsSetup });
        }
        if (path === "/health" && request.method() === "GET") {
            return this.json(route, 200, { status: "ok", service: "dmx-server-manager", version: "1.1.6" });
        }
        if (path === "/auth/setup" && request.method() === "POST") {
            if (!this.needsSetup) return this.problem(route, 409, "Setup already completed");
            const body = record.body as { username?: unknown; password?: unknown };
            const setupToken = record.headers["x-setup-token"];
            const setupTokenInvalid = setupToken !== undefined
                && (setupToken.length < 32 || setupToken.length > 256 || /\s|[\u0000-\u001f\u007f]/u.test(setupToken));
            if (typeof body?.username !== "string"
                || typeof body.password !== "string"
                || body.password.length < 12
                || setupTokenInvalid) {
                return this.problem(route, 400, "Invalid setup payload");
            }
            this.needsSetup = false;
            this.authenticated = true;
            await this.addSessionCookie(page);
            return this.auth(route, { ...this.user, username: body.username }, 201);
        }
        if (path === "/auth/login" && request.method() === "POST") {
            const body = record.body as { username?: unknown; password?: unknown };
            if (body?.username !== "owner" || body.password !== "Correct-Horse-2026!") {
                return this.problem(route, 401, "Identifiants invalides");
            }
            this.authenticated = true;
            await this.addSessionCookie(page);
            return this.auth(route, this.user);
        }

        const cookie = record.headers.cookie ?? "";
        if (!this.authenticated || !cookie.split(/;\s*/).includes(`${SESSION_COOKIE}=${SESSION_TOKEN}`)) {
            return this.problem(route, 401, "Session requise");
        }
        if (!["GET", "HEAD", "OPTIONS"].includes(request.method())
            && record.headers["x-csrf-token"] !== CSRF_TOKEN) {
            return this.problem(route, 403, "Jeton CSRF invalide");
        }

        if (path === "/auth/me" && request.method() === "GET") return this.auth(route, this.user);
        if (path === "/auth/logout" && request.method() === "POST") {
            this.authenticated = false;
            await page.context().clearCookies({ name: SESSION_COOKIE });
            return this.json(route, 200, { success: true });
        }
        if (path === "/auth/password" && request.method() === "PUT") {
            const body = record.body as { current_password?: unknown; new_password?: unknown };
            if (body?.current_password !== "Correct-Horse-2026!") {
                return this.problem(route, 401, "auth.invalid_current_password");
            }
            if (typeof body.new_password !== "string"
                || body.new_password.length < 12
                || body.new_password === body.current_password) {
                return this.problem(route, 400, "auth.password_too_weak");
            }
            this.user.must_change_password = false;
            this.authenticated = false;
            await page.context().clearCookies({ name: SESSION_COOKIE });
            return this.json(route, 200, { success: true, message: "auth.password_updated" });
        }
        if (path === "/auth/preferences" && request.method() === "PATCH") {
            const body = record.body as { language?: unknown; accent_color?: unknown };
            if (body.language === "fr" || body.language === "en") this.user.language = body.language;
            if (typeof body.accent_color === "string") this.user.accent_color = body.accent_color;
            return this.json(route, 200, this.user);
        }
        if (path === "/auth/sessions" && request.method() === "GET") {
            return this.json(route, 200, [{
                id: "10101010-1010-4010-8010-101010101010",
                browser: "Chromium",
                created_at: NOW,
                last_seen_at: NOW,
                expires_at: "2026-08-13T12:00:00.000Z",
                is_current: true,
            }, {
                id: "20202020-2020-4020-8020-202020202020",
                browser: "Safari",
                created_at: NOW,
                last_seen_at: NOW,
                expires_at: "2026-08-13T12:00:00.000Z",
                is_current: false,
            }]);
        }
        if (path === "/auth/sessions/revoke-others" && request.method() === "POST") {
            return this.json(route, 200, { success: true });
        }
        if (/^\/auth\/sessions\/[0-9a-f-]+$/i.test(path) && request.method() === "DELETE") {
            return this.json(route, 200, { success: true });
        }
        if (this.user.must_change_password) {
            return this.problem(route, 403, "auth.password_change_required", "AUTH_009");
        }
        if (path === "/panel/network" && request.method() === "GET") {
            return this.json(route, 200, {
                advertised_game_host: this.advertisedGameHost,
                version: this.networkVersion,
                updated_at: NOW,
            });
        }
        if (path === "/panel/network" && request.method() === "PUT") {
            const body = record.body as { advertised_game_host?: unknown; expected_version?: unknown };
            if (body.expected_version !== this.networkVersion) return this.problem(route, 409, "Version réseau obsolète");
            this.advertisedGameHost = typeof body.advertised_game_host === "string" ? body.advertised_game_host : null;
            this.networkVersion += 1;
            return this.json(route, 200, {
                advertised_game_host: this.advertisedGameHost,
                version: this.networkVersion,
                updated_at: NOW,
            });
        }
        if (path === "/permissions" && request.method() === "GET") {
            return this.user.role === "owner"
                ? this.json(route, 200, this.permissions)
                : this.problem(route, 403, "Owner requis");
        }
        if (path === "/roles" && request.method() === "GET") {
            return this.json(route, 200, this.roles);
        }
        if (path === "/roles" && request.method() === "POST") {
            if (this.user.role !== "owner") return this.problem(route, 403, "Owner requis");
            const body = record.body as { name?: unknown; permissions?: unknown };
            if (typeof body?.name !== "string" || !Array.isArray(body.permissions)) {
                return this.problem(route, 400, "Rôle invalide");
            }
            const role = ManagedRoleSchema.parse({
                id: crypto.randomUUID(),
                name: body.name.trim(),
                permissions: body.permissions,
                is_system: false,
                created_at: NOW,
                updated_at: NOW,
            });
            this.roles.push(role);
            return this.json(route, 201, role);
        }
        const roleMatch = path.match(/^\/roles\/([^/]+)$/);
        if (roleMatch && request.method() === "PATCH") {
            if (this.user.role !== "owner") return this.problem(route, 403, "Owner requis");
            const index = this.roles.findIndex((role) => role.id === decodeURIComponent(roleMatch[1]!));
            const current = this.roles[index];
            if (!current) return this.problem(route, 404, "Rôle introuvable");
            if (current.is_system) return this.problem(route, 403, "Rôle système immuable");
            const body = record.body as { name?: unknown; permissions?: unknown };
            const updated = ManagedRoleSchema.parse({
                ...current,
                ...(typeof body.name === "string" ? { name: body.name.trim() } : {}),
                ...(Array.isArray(body.permissions) ? { permissions: body.permissions } : {}),
                updated_at: NOW,
            });
            this.roles[index] = updated;
            return this.json(route, 200, updated);
        }
        if (roleMatch && request.method() === "DELETE") {
            if (this.user.role !== "owner") return this.problem(route, 403, "Owner requis");
            const index = this.roles.findIndex((role) => role.id === decodeURIComponent(roleMatch[1]!));
            const current = this.roles[index];
            if (!current) return this.problem(route, 404, "Rôle introuvable");
            if (current.is_system) return this.problem(route, 403, "Rôle système immuable");
            this.roles.splice(index, 1);
            return this.json(route, 200, { success: true });
        }
        if (path === "/users" && request.method() === "GET") {
            const visibleUsers = this.user.role === "owner"
                ? this.users
                : this.users.filter((managedUser) => managedUser.role_id !== "owner");
            return this.json(route, 200, visibleUsers);
        }
        if (path === "/users" && request.method() === "POST") {
            const body = record.body as {
                username?: unknown;
                password?: unknown;
                role_id?: unknown;
                language?: unknown;
            };
            const role = this.roles.find((candidate) => candidate.id === body.role_id);
            if (typeof body?.username !== "string" || typeof body.password !== "string" || !role) {
                return this.problem(route, 400, "Compte invalide");
            }
            if (role.id === "owner" && this.user.role !== "owner") {
                return this.problem(route, 403, "Owner géré par l’Owner uniquement");
            }
            const managedUser = ManagedUserSchema.parse({
                id: crypto.randomUUID(),
                username: body.username.trim(),
                role_id: role.id,
                role_name: role.name,
                is_active: true,
                language: body.language === "en" ? "en" : "fr",
                accent_color: "#3a82f6",
                must_change_password: true,
                last_login_at: null,
                created_at: NOW,
                updated_at: NOW,
            });
            this.users.push(managedUser);
            return this.json(route, 201, managedUser);
        }
        const userMatch = path.match(/^\/users\/([0-9a-f-]+)$/i);
        if (userMatch && request.method() === "PATCH") {
            const index = this.users.findIndex((managedUser) => managedUser.id === userMatch[1]);
            const current = this.users[index];
            if (!current) return this.problem(route, 404, "Compte introuvable");
            if (current.role_id === "owner" && this.user.role !== "owner") {
                return this.problem(route, 403, "Owner géré par l’Owner uniquement");
            }
            const body = record.body as {
                role_id?: unknown;
                is_active?: unknown;
                language?: unknown;
                accent_color?: unknown;
                password?: unknown;
            };
            const targetRole = typeof body.role_id === "string"
                ? this.roles.find((role) => role.id === body.role_id)
                : this.roles.find((role) => role.id === current.role_id);
            if (!targetRole) return this.problem(route, 400, "Rôle invalide");
            const updated = ManagedUserSchema.parse({
                ...current,
                role_id: targetRole.id,
                role_name: targetRole.name,
                ...(typeof body.is_active === "boolean" ? { is_active: body.is_active } : {}),
                ...(body.language === "fr" || body.language === "en" ? { language: body.language } : {}),
                ...(typeof body.accent_color === "string" ? { accent_color: body.accent_color } : {}),
                ...(typeof body.password === "string" ? { must_change_password: true } : {}),
                updated_at: NOW,
            });
            this.users[index] = updated;
            return this.json(route, 200, updated);
        }
        const grantsMatch = path.match(/^\/users\/([0-9a-f-]+)\/instances$/i);
        if (grantsMatch && request.method() === "GET") {
            return this.json(route, 200, this.grants.get(grantsMatch[1]!) ?? []);
        }
        const grantMatch = path.match(/^\/users\/([0-9a-f-]+)\/instances\/([0-9a-f-]+)$/i);
        if (grantMatch && request.method() === "PUT") {
            const body = record.body as { permissions?: unknown };
            const instance = this.instances.find((candidate) => candidate.id === grantMatch[2]);
            if (!instance || !Array.isArray(body.permissions)) return this.problem(route, 400, "Affectation invalide");
            const grant = InstanceGrantSchema.parse({
                instance_id: instance.id,
                instance_name: instance.name,
                permissions: body.permissions,
                created_at: NOW,
            });
            const current = this.grants.get(grantMatch[1]!) ?? [];
            this.grants.set(grantMatch[1]!, [...current.filter((item) => item.instance_id !== instance.id), grant]);
            return this.json(route, 200, grant);
        }
        if (grantMatch && request.method() === "DELETE") {
            const current = this.grants.get(grantMatch[1]!) ?? [];
            this.grants.set(grantMatch[1]!, current.filter((item) => item.instance_id !== grantMatch[2]));
            return this.json(route, 200, { success: true });
        }
        if (path === "/game-profiles" && request.method() === "GET") {
            return this.json(route, 200, this.profiles);
        }
        if (path === "/catalog/theme" && request.method() === "GET") {
            return this.json(route, 200, this.activeTheme, { etag: `"${this.activeTheme.version}"` });
        }
        if (path === "/catalog/theme" && request.method() === "PUT") {
            if (this.user.role !== "owner") return this.problem(route, 403, "Owner requis");
            if (record.headers["if-match"] !== `"${this.activeTheme.version}"`) {
                return this.problem(route, 409, "Version du thème obsolète");
            }
            const selection = ThemeSelectionSchema.safeParse(record.body);
            if (!selection.success) return this.problem(route, 400, "Sélection de thème invalide");
            if (selection.data.kind === "default") {
                this.activeTheme = ActiveThemeSchema.parse({
                    selection: selection.data,
                    tokens: DEFAULT_THEME_TOKENS,
                    assets: { logo: null, preview: null },
                    version: this.activeTheme.version + 1,
                    updated_at: NOW,
                });
            } else {
                const catalogSelection = selection.data;
                const catalogPackage = this.catalogPackages.find((candidate) => (
                    candidate.kind === "theme"
                    && candidate.id === catalogSelection.package_id
                    && candidate.revision === catalogSelection.revision
                ));
                if (!catalogPackage?.theme_tokens) return this.problem(route, 404, "Révision du thème introuvable");
                this.activeTheme = ActiveThemeSchema.parse({
                    selection: catalogSelection,
                    tokens: catalogPackage.theme_tokens,
                    assets: { logo: null, preview: null },
                    version: this.activeTheme.version + 1,
                    updated_at: NOW,
                });
            }
            return this.json(route, 200, this.activeTheme, { etag: `"${this.activeTheme.version}"` });
        }
        if (path === "/catalog" && request.method() === "GET") {
            const kind = url.searchParams.get("kind");
            return this.json(route, 200, kind
                ? this.catalogPackages.filter((catalogPackage) => catalogPackage.kind === kind)
                : this.catalogPackages);
        }
        if (path === "/catalog/import" && request.method() === "POST") {
            if (this.user.role !== "owner") return this.problem(route, 403, "Owner requis");
            const checksum = record.headers["x-dmx-package-sha256"];
            if (!checksum?.match(/^[0-9a-f]{64}$/) || !record.headers["idempotency-key"]) {
                return this.problem(route, 400, "Checksum et idempotence requis");
            }
            const job = JobSchema.parse({
                id: crypto.randomUUID(),
                instance_id: null,
                kind: `catalog.import:${checksum}`,
                state: "queued",
                progress: 0,
                requested_by: this.user.id,
                created_at: NOW,
                interaction: null,
            });
            this.jobs.unshift(job);
            return this.json(route, 202, job);
        }
        const catalogRevisionsMatch = path.match(/^\/catalog\/(steam_profile|theme)\/([^/]+)\/revisions$/);
        if (catalogRevisionsMatch && request.method() === "GET") {
            const kind = catalogRevisionsMatch[1];
            const id = decodeURIComponent(catalogRevisionsMatch[2]!);
            return this.json(route, 200, this.catalogPackages.filter((catalogPackage) => (
                catalogPackage.kind === kind && catalogPackage.id === id
            )));
        }
        const catalogRevisionMatch = path.match(/^\/catalog\/(steam_profile|theme)\/([^/]+)\/revisions\/([1-9][0-9]*)$/);
        if (catalogRevisionMatch && request.method() === "DELETE") {
            if (this.user.role !== "owner") return this.problem(route, 403, "Owner requis");
            const kind = catalogRevisionMatch[1];
            const id = decodeURIComponent(catalogRevisionMatch[2]!);
            const revision = Number(catalogRevisionMatch[3]);
            if (this.activeTheme.selection.kind === "catalog"
                && this.activeTheme.selection.package_id === id
                && this.activeTheme.selection.revision === revision) {
                return this.problem(route, 409, "Thème actif");
            }
            const index = this.catalogPackages.findIndex((catalogPackage) => (
                catalogPackage.kind === kind
                && catalogPackage.id === id
                && catalogPackage.revision === revision
            ));
            if (index < 0) return this.problem(route, 404, "Révision introuvable");
            this.catalogPackages.splice(index, 1);
            return this.json(route, 200, { success: true, message: "catalog.revision_deleted" });
        }
        if (path === "/mods/providers" && request.method() === "GET") {
            return this.user.role === "owner"
                ? this.json(route, 200, { modrinth: { configured: true }, curseforge: { configured: this.curseForgeConfigured } })
                : this.problem(route, 403, "Owner requis");
        }
        if (path === "/mods/providers/curseforge" && request.method() === "PUT") {
            if (this.user.role !== "owner") return this.problem(route, 403, "Owner requis");
            const body = record.body as { api_key?: unknown };
            if (typeof body.api_key !== "string" || body.api_key.length < 16) return this.problem(route, 400, "Clé invalide");
            this.curseForgeConfigured = true;
            return this.json(route, 200, { configured: true });
        }
        if (path === "/mods/providers/curseforge" && request.method() === "DELETE") {
            if (this.user.role !== "owner") return this.problem(route, 403, "Owner requis");
            this.curseForgeConfigured = false;
            return this.json(route, 200, { configured: false });
        }
        if (path === "/webhooks" && request.method() === "GET") {
            return this.user.role === "owner"
                ? this.json(route, 200, this.webhooks)
                : this.problem(route, 403, "Owner requis");
        }
        if (path === "/releases/panel" && request.method() === "GET") {
            return this.user.role === "owner"
                ? this.json(route, 200, this.releaseStatus)
                : this.problem(route, 403, "Owner requis");
        }
        if (path === "/releases/panel/check" && request.method() === "POST") {
            if (this.user.role !== "owner") return this.problem(route, 403, "Owner requis");
            this.releaseStatus = PanelReleaseStatusSchema.parse({ ...this.releaseStatus, checked_at: NOW });
            return this.json(route, 200, this.releaseStatus);
        }
        if (path === "/webhooks" && request.method() === "POST") {
            if (this.user.role !== "owner") return this.problem(route, 403, "Owner requis");
            const body = CreateDiscordWebhookSchema.safeParse(record.body);
            if (!body.success) return this.problem(route, 400, "Webhook Discord invalide");
            const webhook = DiscordWebhookSchema.parse({
                id: "13131313-1313-4313-8313-131313131313",
                name: body.data.name,
                events: body.data.events,
                enabled: body.data.enabled,
                configured: true,
                version: 1,
                last_delivery_at: null,
                last_error_code: null,
                created_at: NOW,
                updated_at: NOW,
            });
            this.webhooks.push(webhook);
            return this.json(route, 201, webhook, { etag: '"1"' });
        }
        const webhookMatch = path.match(/^\/webhooks\/([0-9a-f-]+)$/i);
        if (webhookMatch && request.method() === "PUT") {
            if (this.user.role !== "owner") return this.problem(route, 403, "Owner requis");
            const index = this.webhooks.findIndex((webhook) => webhook.id === webhookMatch[1]);
            const current = this.webhooks[index];
            if (!current) return this.problem(route, 404, "Webhook introuvable");
            if (record.headers["if-match"] !== `"${current.version}"`) return this.problem(route, 409, "Version obsolète");
            const body = UpdateDiscordWebhookSchema.safeParse(record.body);
            if (!body.success) return this.problem(route, 400, "Webhook Discord invalide");
            const updated = DiscordWebhookSchema.parse({
                ...current,
                name: body.data.name,
                events: body.data.events,
                enabled: body.data.enabled,
                configured: current.configured || body.data.url !== undefined,
                version: current.version + 1,
                updated_at: NOW,
            });
            this.webhooks[index] = updated;
            return this.json(route, 200, updated, { etag: `"${updated.version}"` });
        }
        if (webhookMatch && request.method() === "DELETE") {
            if (this.user.role !== "owner") return this.problem(route, 403, "Owner requis");
            const index = this.webhooks.findIndex((webhook) => webhook.id === webhookMatch[1]);
            if (index < 0) return this.problem(route, 404, "Webhook introuvable");
            this.webhooks.splice(index, 1);
            return route.fulfill({ status: 204 });
        }
        const profileRevisionsMatch = path.match(/^\/game-profiles\/([^/]+)\/revisions$/);
        if (profileRevisionsMatch && request.method() === "GET") {
            const id = decodeURIComponent(profileRevisionsMatch[1]!);
            const revisions = this.profileRevisions.get(id);
            return revisions ? this.json(route, 200, revisions) : this.problem(route, 404, "Profil introuvable");
        }
        if (path === "/game-profiles/steam" && request.method() === "POST") {
            const body = CreateSteamProfileSchema.safeParse(record.body);
            if (!body.success) return this.problem(route, 400, "Profil Steam invalide");
            if (this.profiles.some((profile) => profile.id === body.data.id)) return this.problem(route, 409, "Identifiant déjà utilisé");
            const profile = customSteamProfile(body.data.id, 1, body.data.definition);
            this.profiles.push(profile);
            this.profileRevisions.set(profile.id, [profile]);
            return this.json(route, 201, profile, { etag: '"1"' });
        }
        const steamProfileMatch = path.match(/^\/game-profiles\/steam\/([^/]+)$/);
        if (steamProfileMatch && request.method() === "PUT") {
            const id = decodeURIComponent(steamProfileMatch[1]!);
            const index = this.profiles.findIndex((profile) => profile.id === id && profile.kind === "steam_custom");
            const current = this.profiles[index];
            if (!current) return this.problem(route, 404, "Profil introuvable");
            if (record.headers["if-match"] !== `"${current.revision}"`) return this.problem(route, 409, "Révision obsolète");
            const definition = SteamProfileDefinitionSchema.safeParse(record.body);
            if (!definition.success || definition.data.app_id !== current.steam_profile?.app_id) return this.problem(route, 400, "Profil Steam invalide");
            const revised = customSteamProfile(id, current.revision + 1, definition.data);
            this.profiles[index] = revised;
            this.profileRevisions.set(id, [...(this.profileRevisions.get(id) ?? []), revised]);
            return this.json(route, 201, revised, { etag: `"${revised.revision}"` });
        }
        if (steamProfileMatch && request.method() === "DELETE") {
            const id = decodeURIComponent(steamProfileMatch[1]!);
            const index = this.profiles.findIndex((profile) => profile.id === id && profile.kind === "steam_custom");
            if (index < 0) return this.problem(route, 404, "Profil introuvable");
            this.profiles.splice(index, 1);
            this.profileRevisions.delete(id);
            return this.json(route, 200, { success: true });
        }
        if (path === "/schedules" && request.method() === "GET") {
            const instanceId = url.searchParams.get("instance_id");
            return this.json(route, 200, this.schedules.filter((schedule) => schedule.instance_id === instanceId));
        }
        if (path === "/schedules" && request.method() === "POST") {
            const body = CreateScheduleSchema.safeParse(record.body);
            if (!body.success) return this.problem(route, 400, "Tâche invalide");
            const schedule = ScheduleSchema.parse({
                ...body.data,
                id: "12121212-1212-4212-8212-121212121212",
                next_run_at: body.data.enabled ? "2026-07-13T13:00:00.000Z" : null,
                last_run_at: null,
                last_job_id: null,
                version: 1,
                created_by: this.user.id,
                requested_by: this.user.id,
                created_at: NOW,
                updated_at: NOW,
            });
            this.schedules.push(schedule);
            return this.json(route, 201, schedule, { etag: '"1"' });
        }
        const scheduleMatch = path.match(/^\/schedules\/([0-9a-f-]+)$/i);
        if (scheduleMatch && request.method() === "PUT") {
            const index = this.schedules.findIndex((schedule) => schedule.id === scheduleMatch[1]);
            const current = this.schedules[index];
            if (!current) return this.problem(route, 404, "Tâche introuvable");
            if (record.headers["if-match"] !== `"${current.version}"`) return this.problem(route, 409, "Version obsolète");
            const body = UpdateScheduleSchema.safeParse(record.body);
            if (!body.success) return this.problem(route, 400, "Tâche invalide");
            const updated = ScheduleSchema.parse({
                ...current,
                ...body.data,
                next_run_at: body.data.enabled ? "2026-07-14T10:00:00.000Z" : null,
                version: current.version + 1,
                requested_by: this.user.id,
                updated_at: NOW,
            });
            this.schedules[index] = updated;
            return this.json(route, 200, updated, { etag: `"${updated.version}"` });
        }
        if (scheduleMatch && request.method() === "DELETE") {
            const index = this.schedules.findIndex((schedule) => schedule.id === scheduleMatch[1]);
            if (index < 0) return this.problem(route, 404, "Tâche introuvable");
            this.schedules.splice(index, 1);
            return this.json(route, 200, { success: true });
        }
        if (path === "/servers" && request.method() === "GET") {
            return this.json(route, 200, this.instances);
        }
        if (path === "/servers" && request.method() === "POST") {
            const body = record.body as {
                name?: unknown;
                profile_id?: unknown;
                settings?: unknown;
                auto_start?: unknown;
            };
            if (typeof body?.name !== "string" || typeof body.profile_id !== "string" || typeof body.settings !== "object") {
                return this.problem(route, 400, "Instance invalide");
            }
            const instance = InstanceSchema.parse({
                id: "33333333-3333-4333-8333-333333333333",
                name: body.name,
                profile_id: body.profile_id,
                profile_revision: this.profiles.find((profile) => profile.id === body.profile_id)?.revision ?? 1,
                settings: body.settings,
                config_version: 1,
                installation_state: "not_installed",
                installed_version: null,
                installed_build: null,
                desired_state: "stopped",
                runtime_state: "stopped",
                managed: true,
                auto_start: body.auto_start === true,
                watchdog_enabled: true,
                created_at: NOW,
                updated_at: NOW,
            });
            this.instances.push(instance);
            return this.json(route, 201, instance, { etag: '"1"' });
        }
        if (path === "/files" && request.method() === "GET") {
            const directory = url.searchParams.get("path") ?? "";
            return this.json(route, 200, { items: directory === "" ? this.files : [] });
        }
        if (/^\/servers\/[0-9a-f-]+\/mods$/i.test(path) && request.method() === "GET") {
            return this.json(route, 200, { items: [] });
        }
        const providerModMatch = path.match(/^\/servers\/([0-9a-f-]+)\/mods\/provider$/i);
        if (providerModMatch && request.method() === "POST") {
            const body = record.body as { provider?: unknown; project_id?: unknown; version_id?: unknown };
            if (!matchesProvider(body.provider) || typeof body.project_id !== "string" || typeof body.version_id !== "string") {
                return this.problem(route, 400, "Mod fournisseur invalide");
            }
            return this.json(route, 201, InstalledModSchema.parse({
                id: "14141414-1414-4414-8414-141414141414",
                instance_id: providerModMatch[1],
                source: body.provider,
                display_name: "provider-example.jar",
                checksum_sha256: "c".repeat(64),
                size_bytes: 4_096,
                provider_project_id: body.project_id,
                provider_version_id: body.version_id,
                enabled: true,
                created_at: NOW,
            }));
        }
        if (path === "/files/text" && request.method() === "GET") {
            return this.json(route, 200, { content: "motd=Serveur Dmx\nmax-players=20\n" });
        }
        if (path === "/files/text" && request.method() === "PUT") {
            const body = record.body as { content?: unknown };
            return this.json(route, 200, { bytes_written: typeof body?.content === "string" ? body.content.length : 0 });
        }
        if (path === "/files/content" && request.method() === "PUT") {
            return this.json(route, 201, { bytes_written: request.postDataBuffer()?.byteLength ?? 0 });
        }
        if (path === "/files/directories" && request.method() === "POST") {
            return this.json(route, 201, { success: true });
        }
        if (path === "/files" && request.method() === "DELETE") {
            const target = url.searchParams.get("path");
            const index = this.files.findIndex((item) => item.path === target);
            if (index >= 0) this.files.splice(index, 1);
            return this.json(route, 200, { success: true });
        }
        const importMatch = path.match(/^\/servers\/([0-9a-f-]+)\/imports\/(zip|copy|attach)$/i);
        if (importMatch && request.method() === "POST") {
            const instanceId = importMatch[1]!;
            const mode = importMatch[2]!;
            if (mode === "attach" && this.user.role !== "owner") return this.problem(route, 403, "Owner requis");
            const waitingBedrock = this.jobs.find((job) => job.instance_id === instanceId
                && job.kind === "install"
                && job.state === "waiting_for_user"
                && job.interaction?.kind === "bedrock_archive_upload");
            if (waitingBedrock && mode === "zip") {
                if (!record.headers["x-dmx-archive-sha256"]) return this.problem(route, 400, "SHA-256 requis");
                waitingBedrock.state = "queued";
                waitingBedrock.interaction = null;
                return this.json(route, 202, JobSchema.parse(waitingBedrock));
            }
            const job = JobSchema.parse({
                id: crypto.randomUUID(),
                instance_id: instanceId,
                kind: `import_${mode}`,
                state: "queued",
                progress: 0,
                requested_by: this.user.id,
                created_at: NOW,
                interaction: null,
            });
            this.jobs.unshift(job);
            return this.json(route, 202, job);
        }
        if (path === "/backups" && request.method() === "GET") {
            const instanceId = url.searchParams.get("instance_id");
            return this.json(route, 200, this.backups.filter((backup) => backup.instance_id === instanceId));
        }
        if (path === "/activity/summary" && request.method() === "GET") {
            return this.json(route, 200, {
                active_jobs: this.jobs.filter((job) => ["queued", "running", "waiting_for_user"].includes(job.state)).length,
                waiting_for_user: this.jobs.filter((job) => job.state === "waiting_for_user").length,
                failed_jobs_24h: this.jobs.filter((job) => ["failed", "interrupted"].includes(job.state)).length,
                crashed_servers: this.instances.filter((instance) => instance.runtime_state === "crashed").length,
                config_conflicts: 0,
            });
        }
        if (path === "/activity/jobs" && request.method() === "GET") {
            const state = url.searchParams.get("state");
            const instanceId = url.searchParams.get("instance_id");
            const items = this.jobs.filter((job) => (!state || job.state === state)
                && (!instanceId || job.instance_id === instanceId));
            return this.json(route, 200, { items, next_cursor: null });
        }
        if (path === "/audit" && request.method() === "GET") {
            return this.json(route, 200, {
                items: [{
                    id: 1,
                    actor_user_id: this.user.id,
                    actor_username: this.user.username,
                    action: "server.updated",
                    resource_type: "instance",
                    resource_id: this.instances[0]?.id ?? null,
                    outcome: "success",
                    metadata: {},
                    created_at: NOW,
                }],
                next_before_id: null,
            });
        }
        if (path === "/jobs" && request.method() === "GET") {
            return this.json(route, 200, this.jobs);
        }
        const jobCancelMatch = path.match(/^\/jobs\/([0-9a-f-]+)\/cancel$/i);
        if (jobCancelMatch && request.method() === "POST") {
            const job = this.jobs.find((candidate) => candidate.id === jobCancelMatch[1]);
            if (!job) return this.problem(route, 404, "Job introuvable");
            if (job.kind !== "install" || !["queued", "running", "waiting_for_user"].includes(job.state)) {
                return this.problem(route, 409, "Job non annulable");
            }
            job.state = "cancelled";
            job.progress = 100;
            job.error_code = "cancelled_by_user";
            job.error_message = "jobs.cancelled";
            job.finished_at = NOW;
            job.interaction = null;
            return this.json(route, 202, JobSchema.parse(job));
        }
        const jobMatch = path.match(/^\/jobs\/([0-9a-f-]+)$/i);
        if (jobMatch && request.method() === "GET") {
            const job = this.jobs.find((candidate) => candidate.id === jobMatch[1]);
            if (job?.kind.startsWith("catalog.import:") && job.state === "queued") {
                job.state = "succeeded";
                job.progress = 100;
                job.started_at = NOW;
                job.finished_at = NOW;
            }
            return job ? this.json(route, 200, job) : this.problem(route, 404, "Job introuvable");
        }
        if (path === "/backups" && request.method() === "POST") {
            return this.json(route, 202, JobSchema.parse({
                id: "aaaaaaaa-1111-4111-8111-111111111111",
                instance_id: (record.body as { instance_id?: string }).instance_id,
                kind: "backup.create",
                state: "queued",
                progress: 0,
                requested_by: this.user.id,
                created_at: NOW,
                interaction: null,
            }));
        }
        const backupRestoreMatch = path.match(/^\/backups\/([0-9a-f-]+)\/restore$/i);
        if (backupRestoreMatch && request.method() === "POST") {
            const backup = this.backups.find((item) => item.id === backupRestoreMatch[1]);
            return this.json(route, 202, JobSchema.parse({
                id: "bbbbbbbb-1111-4111-8111-111111111111",
                instance_id: backup?.instance_id,
                kind: `backup.restore:${backupRestoreMatch[1]}`,
                state: "queued",
                progress: 0,
                requested_by: this.user.id,
                created_at: NOW,
                interaction: null,
            }));
        }
        const backupMatch = path.match(/^\/backups\/([0-9a-f-]+)$/i);
        if (backupMatch && request.method() === "DELETE") {
            const index = this.backups.findIndex((item) => item.id === backupMatch[1]);
            if (index >= 0) this.backups.splice(index, 1);
            return this.json(route, 200, { success: true });
        }
        const metricsMatch = path.match(/^\/servers\/([0-9a-f-]+)\/metrics$/i);
        if (metricsMatch && request.method() === "GET") {
            return this.json(route, 200, {
                server_id: metricsMatch[1],
                period: url.searchParams.get("period") ?? "1d",
                points: this.metrics,
            });
        }
        if (path === "/metrics/current" && request.method() === "GET") {
            const latest = this.metrics.at(-1);
            return this.json(route, 200, {
                items: latest ? this.instances.map((instance) => ({
                    server_id: instance.id,
                    cpu_usage: latest.cpu_usage,
                    memory_bytes: latest.memory_bytes,
                    disk_bytes: latest.disk_bytes,
                    uptime_seconds: latest.uptime_seconds,
                    player_count: latest.player_count,
                    recorded_at: latest.recorded_at,
                })) : [],
            });
        }
        if (path === "/metrics/system" && request.method() === "GET") {
            return this.json(route, 200, {
                cpu_usage: 22.4,
                memory_used_bytes: 8_589_934_592,
                memory_total_bytes: 17_179_869_184,
                disk_used_bytes: 128_849_018_880,
                disk_total_bytes: 536_870_912_000,
                network_receive_bytes_per_second: 1_048_576,
                network_transmit_bytes_per_second: 262_144,
                recorded_at: NOW,
            });
        }

        const playersMatch = path.match(/^\/servers\/([0-9a-f-]+)\/players$/i);
        if (playersMatch && request.method() === "GET") {
            return this.json(route, 200, {
                instance_id: playersMatch[1],
                online_count: 1,
                detection: "console_log",
                access_mode: "native_files",
                players: [{
                    player_key: "id:76561198000000000",
                    display_name: "DmxPlayer",
                    external_id: "76561198000000000",
                    source: "steam",
                    online: true,
                    first_seen_at: NOW,
                    last_seen_at: NOW,
                    connected_at: NOW,
                    disconnected_at: null,
                }],
            });
        }

        const configFilesMatch = path.match(/^\/servers\/([0-9a-f-]+)\/config-files$/i);
        if (configFilesMatch && request.method() === "GET") {
            const queuedContent = (nativePath: string): string | null => {
                const queuedRequest = [...this.requests].reverse().find((item) => item.method === "PUT"
                    && item.path === `/servers/${configFilesMatch[1]}/config-files/text`
                    && new URLSearchParams(item.search).get("path") === nativePath);
                return typeof queuedRequest?.body === "object"
                    && queuedRequest.body !== null
                    && "content" in queuedRequest.body
                    && typeof queuedRequest.body.content === "string"
                    ? queuedRequest.body.content
                    : null;
            };
            const accessQueued = queuedContent("data/adminlist.txt");
            const configurationQueued = queuedContent("game/Server/config.json");
            return this.json(route, 200, {
                items: [{
                    path: "game/Server/config.json",
                    category: "configuration",
                    format: "json",
                    exists: true,
                    size_bytes: 196,
                    modified_at: NOW,
                    sha256: "c".repeat(64),
                    queued_change: configurationQueued === null ? null : {
                        id: "14141414-1414-4414-8414-141414141414",
                        status: "pending",
                        content_sha256: "d".repeat(64),
                        error_code: null,
                        queued_at: NOW,
                    },
                }, {
                    path: "data/adminlist.txt",
                    category: "access",
                    format: "text",
                    exists: true,
                    size_bytes: 18,
                    modified_at: NOW,
                    sha256: "a".repeat(64),
                    queued_change: accessQueued === null ? null : {
                        id: "13131313-1313-4313-8313-131313131313",
                        status: "pending",
                        content_sha256: "b".repeat(64),
                        error_code: null,
                        queued_at: NOW,
                    },
                }],
                pending_count: Number(accessQueued !== null) + Number(configurationQueued !== null),
            });
        }
        const configTextMatch = path.match(/^\/servers\/([0-9a-f-]+)\/config-files\/text$/i);
        if (configTextMatch && request.method() === "GET") {
            const nativePath = url.searchParams.get("path");
            const configuration = nativePath === "game/Server/config.json";
            return this.json(route, 200, {
                file: {
                    path: nativePath,
                    category: configuration ? "configuration" : "access",
                    format: configuration ? "json" : "text",
                    exists: true,
                    size_bytes: configuration ? 196 : 18,
                    modified_at: NOW,
                    sha256: (configuration ? "c" : "a").repeat(64),
                    queued_change: null,
                },
                content: configuration
                    ? "{\n  \"Version\": 4,\n  \"ServerName\": \"Hytale Server\",\n  \"MOTD\": \"Bienvenue\",\n  \"Password\": \"\",\n  \"MaxPlayers\": 100,\n  \"Defaults\": { \"World\": \"default\", \"GameMode\": \"Adventure\" },\n  \"Modules\": {}\n}\n"
                    : "76561198000000000",
                queued_content: null,
            });
        }
        if (configTextMatch && request.method() === "PUT") {
            const body = record.body as { content?: unknown };
            const nativePath = url.searchParams.get("path");
            const configuration = nativePath === "game/Server/config.json";
            return this.json(route, 200, {
                file: {
                    path: nativePath,
                    category: configuration ? "configuration" : "access",
                    format: configuration ? "json" : "text",
                    exists: true,
                    size_bytes: typeof body.content === "string" ? body.content.length : 0,
                    modified_at: NOW,
                    sha256: (configuration ? "c" : "a").repeat(64),
                    queued_change: {
                        id: configuration ? "14141414-1414-4414-8414-141414141414" : "13131313-1313-4313-8313-131313131313",
                        status: "pending",
                        content_sha256: (configuration ? "d" : "b").repeat(64),
                        error_code: null,
                        queued_at: NOW,
                    },
                },
                content: configuration ? "{}" : "76561198000000000",
                queued_content: typeof body.content === "string" ? body.content : "",
            });
        }
        if (configTextMatch && request.method() === "DELETE") {
            return this.json(route, 200, { success: true, message: "config_files.cancelled" });
        }

        const consoleMatch = path.match(/^\/servers\/([0-9a-f-]+)\/console$/i);
        if (consoleMatch && request.method() === "POST") {
            return this.json(route, 202, { accepted: true });
        }

        const connectionMatch = path.match(/^\/servers\/([0-9a-f-]+)\/connection$/i);
        if (connectionMatch && request.method() === "GET") {
            const instance = this.instances.find((candidate) => candidate.id === connectionMatch[1]);
            const profile = this.profiles.find((candidate) => candidate.id === instance?.profile_id);
            if (!instance || !profile) return this.problem(route, 404, "Instance introuvable");
            const endpoints = profile.ports.map((port, index) => {
                const configured = instance.settings[port.name];
                const value = typeof configured === "number" ? configured : port.default;
                return {
                    name: port.name,
                    protocol: port.protocol,
                    port: value,
                    primary: index === 0,
                    address: this.advertisedGameHost ? `${this.advertisedGameHost}:${value}` : null,
                };
            });
            return this.json(route, 200, {
                configured: this.advertisedGameHost !== null,
                host: this.advertisedGameHost,
                connection_type: instance.profile_id === "valheim" ? "steam" : "direct",
                help_key: instance.profile_id === "valheim" ? "connection.help.steam" : "connection.help.minecraft_java",
                endpoints,
            });
        }

        const updateStatusMatch = path.match(/^\/servers\/([0-9a-f-]+)\/update-status$/i);
        if (updateStatusMatch && request.method() === "GET") {
            const instance = this.instances.find((candidate) => candidate.id === updateStatusMatch[1]);
            if (!instance) return this.problem(route, 404, "Instance introuvable");
            const availableVersion = this.updateAvailable && instance.installed_version
                ? `${instance.installed_version}.next`
                : instance.installed_version;
            const availableBuild = this.updateAvailable && !instance.installed_version && instance.installed_build
                ? `${instance.installed_build}1`
                : instance.installed_build;
            return this.json(route, 200, {
                state: instance.installation_state !== "installed"
                    ? "not_installed"
                    : this.updateAvailable ? "update_available" : "up_to_date",
                installed_version: instance.installed_version,
                installed_build: instance.installed_build,
                available_version: availableVersion,
                available_build: availableBuild,
                checked_at: NOW,
            });
        }

        const serverMatch = path.match(/^\/servers\/([0-9a-f-]+)$/i);
        if (serverMatch && request.method() === "GET") {
            const instance = this.instances.find((candidate) => candidate.id === serverMatch[1]);
            return instance
                ? this.json(route, 200, instance, { etag: `"${instance.config_version}"` })
                : this.problem(route, 404, "Instance introuvable");
        }
        const secretsMatch = path.match(/^\/servers\/([0-9a-f-]+)\/secrets$/i);
        if (secretsMatch && request.method() === "GET") {
            return this.json(route, 200, { items: [{ name: "server_password", configured: true }] });
        }
        const actionMatch = path.match(/^\/servers\/([0-9a-f-]+)\/actions\/([a-z_]+)$/i);
        if (actionMatch && request.method() === "POST") {
            const job = JobSchema.parse({
                id: "44444444-4444-4444-8444-444444444444",
                instance_id: actionMatch[1],
                kind: actionMatch[2],
                state: "queued",
                progress: 0,
                requested_by: this.user.id,
                created_at: NOW,
                interaction: null,
            });
            const existingIndex = this.jobs.findIndex((candidate) => candidate.id === job.id);
            if (existingIndex >= 0) this.jobs.splice(existingIndex, 1);
            this.jobs.unshift(job);
            return this.json(route, 202, job);
        }
        if (path === "/events" && request.method() === "GET") {
            const serverId = url.searchParams.get("server_id");
            const event = JSON.stringify({
                type: "server.log",
                server_id: serverId,
                payload: {
                    stream: "stdout",
                    message: "\u001b[32mServeur prêt\u001b[0m <img src=x onerror=alert(1)>",
                },
                created_at: NOW,
            });
            const deviceAuthorization = JSON.stringify({
                type: "job.waiting_for_user",
                server_id: serverId,
                payload: {
                    job_id: "56565656-5656-4565-8565-565656565656",
                    interaction: {
                        kind: "oauth_device",
                        verification_uri: "https://oauth.accounts.hytale.com/oauth2/device/verify?user_code=x6nimECK",
                        user_code: "x6nimECK",
                    },
                },
                created_at: NOW,
            });
            // `route.fulfill` closes its response body immediately. Keep the retry
            // interval long so the finite E2E fixture behaves like the production
            // long-lived SSE stream instead of replaying the same event in a loop.
            const body = serverId
                ? `retry: 60000\nid: e2e-log-1\nevent: server.log\ndata: ${event}\n\n${this.hytaleDeviceAuthorization ? `id: e2e-device-1\nevent: job.waiting_for_user\ndata: ${deviceAuthorization}\n\n` : ""}`
                : "retry: 60000\n\n";
            return route.fulfill({
                status: 200,
                contentType: "text/event-stream",
                headers: { "cache-control": "no-cache" },
                body,
            });
        }

        return this.problem(route, 501, `Route E2E non simulée: ${request.method()} ${path}`);
    }

    private auth(route: Route, user: UserInfo, status = 200): Promise<void> {
        const payload = AuthResponseSchema.parse({ user, csrf_token: CSRF_TOKEN });
        return this.json(route, status, payload);
    }

    private json(route: Route, status: number, body: unknown, headers: Record<string, string> = {}): Promise<void> {
        return route.fulfill({
            status,
            contentType: "application/json; charset=utf-8",
            headers: { "cache-control": "no-store", ...headers },
            body: JSON.stringify(body),
        });
    }

    private problem(route: Route, status: number, detail: string, code?: string): Promise<void> {
        return route.fulfill({
            status,
            contentType: "application/problem+json",
            body: JSON.stringify({
                type: "about:blank",
                title: status === 401 ? "Unauthorized" : status === 403 ? "Forbidden" : "E2E API error",
                status,
                detail,
                trace_id: `e2e-${status}`,
                ...(code ? { code } : {}),
            }),
        });
    }
}
