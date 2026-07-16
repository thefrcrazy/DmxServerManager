import { z } from "zod";
import type { components } from "../generated/api";

export const ProblemDetailsSchema = z.object({
    type: z.string().default("about:blank"),
    title: z.string(),
    status: z.number().int(),
    detail: z.string().optional(),
    code: z.string().optional(),
    trace_id: z.string().optional(),
});

export type ProblemDetails = z.infer<typeof ProblemDetailsSchema>;

export const SuccessResponseSchema = z.object({
    success: z.boolean(),
    message: z.string().optional(),
});

export const UserInfoSchema = z.object({
    id: z.string(),
    username: z.string(),
    role: z.string(),
    permissions: z.array(z.string()),
    accent_color: z.string(),
    must_change_password: z.boolean(),
});

export const AuthResponseSchema = z.object({
    user: UserInfoSchema,
    csrf_token: z.string(),
});

export const SetupStatusSchema = z.object({
    needs_setup: z.boolean(),
});

export type UserInfo = z.infer<typeof UserInfoSchema>;
export type AuthResponse = z.infer<typeof AuthResponseSchema>;
export type SetupStatus = z.infer<typeof SetupStatusSchema>;

export const JsonSchemaPropertySchema = z.object({
    type: z.enum(["string", "integer", "number", "boolean", "array", "object"]),
    title: z.string().optional(),
    description: z.string().optional(),
    default: z.unknown().optional(),
    const: z.unknown().optional(),
    enum: z.array(z.union([z.string(), z.number(), z.boolean()])).optional(),
    minimum: z.number().optional(),
    maximum: z.number().optional(),
    minLength: z.number().int().nonnegative().optional(),
    maxLength: z.number().int().nonnegative().optional(),
    minItems: z.number().int().nonnegative().optional(),
    maxItems: z.number().int().nonnegative().optional(),
    pattern: z.string().optional(),
    format: z.string().optional(),
    secret: z.boolean().optional(),
    writeOnly: z.boolean().optional(),
    items: z.unknown().optional(),
}).catchall(z.unknown());

export const PortSpecSchema = z.object({
    name: z.string(),
    protocol: z.enum(["tcp", "udp"]),
    default: z.number().int().min(1).max(65_535),
    adjacent_to: z.string().nullable().optional(),
});

export const StopStrategySchema = z.discriminatedUnion("kind", [
    z.object({ kind: z.literal("stdin"), command: z.string(), timeout_seconds: z.number().int().positive() }).strict(),
    z.object({ kind: z.literal("interrupt"), timeout_seconds: z.number().int().positive() }).strict(),
    z.object({ kind: z.literal("terminate"), timeout_seconds: z.number().int().positive() }).strict(),
]);

export const SteamExecutableSchema = z.object({
    linux_x86_64: z.string().nullable(),
    windows_x86_64: z.string().nullable(),
}).strict();

export const SteamProfileSchema = z.object({
    app_id: z.number().int().min(1).max(4_294_967_295),
    branch: z.string().nullable(),
    executable: SteamExecutableSchema,
    arguments: z.array(z.string()),
    ports: z.array(PortSpecSchema),
    save_paths: z.array(z.string()),
    ready_log_pattern: z.string().nullable(),
    stop_strategy: StopStrategySchema,
}).strict();

export const GameProfileSchema = z.object({
    id: z.string(),
    revision: z.number().int().positive(),
    name: z.string(),
    description: z.string(),
    kind: z.enum(["builtin", "steam_custom"]),
    platforms: z.array(z.enum(["linux-x64", "windows-x64"])),
    capabilities: z.array(z.string()),
    ports: z.array(PortSpecSchema),
    lifecycle: z.object({
        stop: StopStrategySchema,
        ready_log_pattern: z.string().nullable().optional(),
    }),
    settings_schema: z.object({
        type: z.literal("object").optional(),
        additionalProperties: z.boolean().optional(),
        required: z.array(z.string()).default([]),
        properties: z.record(z.string(), JsonSchemaPropertySchema).default({}),
    }),
    ui_schema: z.record(z.string(), z.unknown()),
    steam_profile: SteamProfileSchema.nullable().optional(),
}).strict();

export type JsonSchemaProperty = z.infer<typeof JsonSchemaPropertySchema>;
export type GameProfile = z.infer<typeof GameProfileSchema>;
export type SteamProfile = z.infer<typeof SteamProfileSchema>;

const ProfileCatalogVersionSchema = z.string().min(1).max(96).regex(/^[A-Za-z0-9._+-]+$/);

export const ProfileVersionCatalogSchema = z.object({
    profile_id: z.string(),
    game_versions: z.array(ProfileCatalogVersionSchema).max(512),
    selected_game_version: ProfileCatalogVersionSchema.nullable(),
    loader_versions: z.array(ProfileCatalogVersionSchema).max(512),
}).strict();

export type ProfileVersionCatalog = z.infer<typeof ProfileVersionCatalogSchema>;

export const InstallationStateSchema = z.enum([
    "not_installed",
    "installing",
    "installed",
    "updating",
    "failed",
]);

export const DesiredStateSchema = z.enum(["running", "stopped"]);
export const RuntimeStateSchema = z.enum([
    "stopped",
    "starting",
    "running",
    "stopping",
    "crashed",
    "unknown",
]);

export const InstanceSchema = z.object({
    id: z.string().uuid(),
    name: z.string(),
    profile_id: z.string(),
    profile_revision: z.number().int().positive(),
    settings: z.record(z.string(), z.unknown()),
    config_version: z.number().int().positive(),
    installation_state: InstallationStateSchema,
    installed_version: z.string().nullable(),
    installed_build: z.string().nullable(),
    desired_state: DesiredStateSchema,
    runtime_state: RuntimeStateSchema,
    managed: z.boolean(),
    auto_start: z.boolean(),
    watchdog_enabled: z.boolean(),
    created_at: z.string(),
    updated_at: z.string(),
});

export type Instance = z.infer<typeof InstanceSchema>;
export type InstallationState = z.infer<typeof InstallationStateSchema>;
export type RuntimeState = z.infer<typeof RuntimeStateSchema>;

export const JobStateSchema = z.enum([
    "queued",
    "running",
    "waiting_for_user",
    "succeeded",
    "failed",
    "cancelled",
    "interrupted",
]);

const officialHytaleUri = z.url().refine((value) => {
    const uri = new URL(value);
    return uri.protocol === "https:"
        && uri.hostname === "accounts.hytale.com"
        && uri.port === ""
        && uri.pathname === "/device"
        && uri.username === ""
        && uri.password === ""
        && uri.hash === ""
        && [...uri.searchParams.keys()].every((key) => key === "user_code");
});

export const HytaleDeviceInteractionSchema = z.object({
    kind: z.literal("oauth_device"),
    verification_uri: officialHytaleUri,
    user_code: z.string().min(4).max(32).regex(/^[A-Z0-9-]+$/).nullable(),
}).strict().superRefine((value, context) => {
    const queryCode = new URL(value.verification_uri).searchParams.get("user_code");
    if (queryCode !== value.user_code) {
        context.addIssue({
            code: "custom",
            path: ["verification_uri"],
            message: "hytale_user_code_mismatch",
        });
    }
});

const bedrockUploadPath = z.string().regex(
    /^\/api\/v1\/servers\/[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\/imports\/zip$/i,
);
export const BedrockArchiveInteractionSchema = z.object({
    kind: z.literal("bedrock_archive_upload"),
    instance_id: z.string().uuid(),
    version: z.string().min(1).max(128).nullable(),
    method: z.literal("POST"),
    path: bedrockUploadPath,
    required_sha256_header: z.literal("x-dmx-archive-sha256"),
    max_bytes: z.literal(4 * 1024 * 1024 * 1024),
}).strict().refine(
    (value) => value.path === `/api/v1/servers/${value.instance_id}/imports/zip`,
    { path: ["path"], message: "instance_path_mismatch" },
);

export const JobInteractionSchema = z.union([
    HytaleDeviceInteractionSchema,
    BedrockArchiveInteractionSchema,
]);

export const JobSchema = z.object({
    id: z.string().uuid(),
    instance_id: z.string().uuid().nullable().optional(),
    kind: z.string(),
    state: JobStateSchema,
    progress: z.number().int().min(0).max(100),
    requested_by: z.string(),
    error_code: z.string().nullable().optional(),
    error_message: z.string().nullable().optional(),
    created_at: z.string(),
    started_at: z.string().nullable().optional(),
    finished_at: z.string().nullable().optional(),
    interaction: JobInteractionSchema.nullable(),
});

export type Job = z.infer<typeof JobSchema>;
export type JobInteraction = z.infer<typeof JobInteractionSchema>;

export const SecretStatusSchema = z.object({
    name: z.string(),
    configured: z.boolean(),
});

export const SecretStatusListSchema = z.object({
    items: z.array(SecretStatusSchema),
});

export const HealthResponseSchema = z.object({
    status: z.enum(["ok", "unavailable"]),
    service: z.literal("dmx-server-manager"),
    version: z.string(),
});

export type HealthResponse = z.infer<typeof HealthResponseSchema>;

export const EventEnvelopeSchema = z.object({
    type: z.string(),
    server_id: z.string().uuid().nullable().optional(),
    payload: z.unknown(),
    created_at: z.string().optional(),
});

export type EventEnvelope = z.infer<typeof EventEnvelopeSchema>;

export const PermissionIdSchema = z.enum([
    "audit.read",
    "chat.read",
    "chat.write",
    "job.read",
    "mods.manage",
    "notifications.read",
    "profile.manage",
    "profile.read",
    "schedule.manage",
    "server.backup",
    "server.backup.read",
    "server.console.read",
    "server.console.write",
    "server.create",
    "server.delete",
    "server.files.read",
    "server.files.write",
    "server.kill",
    "server.read",
    "server.start",
    "server.stop",
    "server.update",
    "server.update_game",
    "user.create",
    "user.read",
    "user.update",
]);

export const InstancePermissionIdSchema = z.enum([
    "job.read",
    "mods.manage",
    "schedule.manage",
    "server.backup",
    "server.backup.read",
    "server.console.read",
    "server.console.write",
    "server.files.read",
    "server.files.write",
    "server.kill",
    "server.read",
    "server.start",
    "server.stop",
    "server.update",
    "server.update_game",
]);

export const PermissionDescriptionSchema: z.ZodType<components["schemas"]["Permission"]> = z.object({
    id: PermissionIdSchema,
    high_risk: z.boolean(),
    instance_scoped: z.boolean(),
}).strict();

export const ManagedRoleSchema: z.ZodType<components["schemas"]["Role"]> = z.object({
    id: z.string().min(1),
    name: z.string().min(1),
    permissions: z.array(z.union([PermissionIdSchema, z.literal("*")])),
    is_system: z.boolean(),
    created_at: z.string(),
    updated_at: z.string(),
}).strict();

export const ManagedUserSchema: z.ZodType<components["schemas"]["ManagedUser"]> = z.object({
    id: z.string().uuid(),
    username: z.string(),
    role_id: z.string(),
    role_name: z.string(),
    is_active: z.boolean(),
    language: z.enum(["fr", "en"]),
    accent_color: z.string().regex(/^#[0-9a-f]{6}$/i),
    must_change_password: z.boolean(),
    last_login_at: z.string().nullable(),
    created_at: z.string(),
    updated_at: z.string(),
}).strict();

export const InstanceGrantSchema: z.ZodType<components["schemas"]["InstanceGrant"]> = z.object({
    instance_id: z.string().uuid(),
    instance_name: z.string(),
    permissions: z.array(InstancePermissionIdSchema),
    created_at: z.string(),
}).strict();

export type PermissionDescription = z.infer<typeof PermissionDescriptionSchema>;
export type PermissionId = z.infer<typeof PermissionIdSchema>;
export type InstancePermissionId = z.infer<typeof InstancePermissionIdSchema>;
export type ManagedRole = z.infer<typeof ManagedRoleSchema>;
export type ManagedUser = z.infer<typeof ManagedUserSchema>;
export type InstanceGrant = z.infer<typeof InstanceGrantSchema>;
