import { z } from "zod";
import {
    BedrockArchiveInteractionSchema,
    GameProfileSchema,
    HytaleDeviceInteractionSchema,
    JobSchema,
    PortSpecSchema,
    StopStrategySchema,
} from "./api";

// Runtime validators mirror the generated OpenAPI contract. The generated
// TypeScript declarations provide compile-time types; Zod keeps API responses
// and mutation payloads fail-closed at runtime.
export const OperationSuccessSchema = z.object({
    success: z.boolean(),
    message: z.string().optional(),
}).strict();
export const AcceptedJobSchema = JobSchema.strict();

export const ActivitySummarySchema = z.object({
    active_jobs: z.number().int().nonnegative(),
    waiting_for_user: z.number().int().nonnegative(),
    failed_jobs_24h: z.number().int().nonnegative(),
    crashed_servers: z.number().int().nonnegative(),
    config_conflicts: z.number().int().nonnegative(),
}).strict();

export const ActivityJobsPageSchema = z.object({
    items: z.array(JobSchema).max(100),
    next_cursor: z.string().uuid().nullable(),
}).strict();

export const AuditEventSchema = z.object({
    id: z.number().int().positive(),
    actor_user_id: z.string().nullable(),
    actor_username: z.string().nullable(),
    action: z.string(),
    resource_type: z.string(),
    resource_id: z.string().nullable(),
    outcome: z.enum(["success", "denied", "failure"]),
    metadata: z.record(z.string(), z.unknown()),
    created_at: z.string(),
}).strict();

export const AuditPageSchema = z.object({
    items: z.array(AuditEventSchema),
    next_before_id: z.number().int().positive().nullable(),
}).strict();

export const NetworkSettingsSchema = z.object({
    advertised_game_host: z.string().nullable(),
    version: z.number().int().positive(),
    updated_at: z.string(),
}).strict();

export const ManagedFileEntrySchema = z.object({
    name: z.string().min(1),
    path: z.string(),
    kind: z.enum(["file", "directory"]),
    size_bytes: z.number().int().nonnegative(),
    modified_at: z.string().nullable(),
}).strict();

export const ManagedFileListSchema = z.object({
    items: z.array(ManagedFileEntrySchema),
}).strict();

export const TextFileSchema = z.object({ content: z.string() }).strict();
export const FileWriteResultSchema = z.object({ bytes_written: z.number().int().nonnegative() }).strict();

export const BackupSchema = z.object({
    id: z.string().uuid(),
    instance_id: z.string().uuid(),
    kind: z.string().min(1),
    status: z.enum(["creating", "ready", "failed"]),
    checksum_sha256: z.string().regex(/^[0-9a-f]{64}$/i).nullable(),
    size_bytes: z.number().int().nonnegative().nullable(),
    created_at: z.string().min(1),
    completed_at: z.string().nullable(),
}).strict();

export const MetricPeriodSchema = z.enum(["1h", "6h", "1d", "7d"]);
export const MetricPointSchema = z.object({
    id: z.string().uuid(),
    cpu_usage: z.number().nonnegative(),
    memory_bytes: z.number().int().nonnegative(),
    disk_bytes: z.number().int().nonnegative(),
    uptime_seconds: z.number().int().nonnegative(),
    player_count: z.number().int().nonnegative().nullable(),
    recorded_at: z.string().min(1),
}).strict();
export const MetricsHistorySchema = z.object({
    server_id: z.string().uuid(),
    period: MetricPeriodSchema,
    points: z.array(MetricPointSchema).max(10_000),
}).strict();
export const CurrentServerMetricSchema = MetricPointSchema.omit({ id: true }).extend({
    server_id: z.string().uuid(),
}).strict();
export const CurrentServerMetricsSchema = z.object({
    items: z.array(CurrentServerMetricSchema),
}).strict();
export const LiveServerMetricSchema = MetricPointSchema.omit({ id: true, recorded_at: true }).strict();
export const SystemMetricsSnapshotSchema = z.object({
    cpu_usage: z.number().nonnegative(),
    memory_used_bytes: z.number().int().nonnegative(),
    memory_total_bytes: z.number().int().nonnegative(),
    disk_used_bytes: z.number().int().nonnegative(),
    disk_total_bytes: z.number().int().nonnegative(),
    network_receive_bytes_per_second: z.number().int().nonnegative(),
    network_transmit_bytes_per_second: z.number().int().nonnegative(),
    recorded_at: z.string().min(1),
}).strict();

export const ConfigFileFormatSchema = z.enum(["json", "properties", "ini", "toml", "yaml", "xml", "lua", "text"]);
export const ConfigFileCategorySchema = z.enum(["configuration", "access"]);
export const ConfigChangeStatusSchema = z.enum(["pending", "applied", "conflict", "failed", "cancelled"]);
export const ConfigChangeSummarySchema = z.object({
    id: z.string().uuid(),
    status: ConfigChangeStatusSchema,
    content_sha256: z.string().regex(/^[0-9a-f]{64}$/i),
    error_code: z.string().nullable(),
    queued_at: z.string().min(1),
}).strict();
export const ConfigFileSummarySchema = z.object({
    path: z.string().min(1).max(1_024),
    category: ConfigFileCategorySchema,
    format: ConfigFileFormatSchema,
    exists: z.boolean(),
    size_bytes: z.number().int().nonnegative(),
    modified_at: z.string().nullable(),
    sha256: z.string().regex(/^[0-9a-f]{64}$/i).nullable(),
    queued_change: ConfigChangeSummarySchema.nullable(),
}).strict();
export const ConfigFileListSchema = z.object({
    items: z.array(ConfigFileSummarySchema).max(512),
    pending_count: z.number().int().nonnegative(),
}).strict();
export const ConfigFileDocumentSchema = z.object({
    file: ConfigFileSummarySchema,
    content: z.string(),
    queued_content: z.string().nullable(),
}).strict();

export const ServerPlayerSchema = z.object({
    player_key: z.string().min(1).max(255),
    display_name: z.string().min(1).max(128),
    external_id: z.string().nullable(),
    source: z.enum(["hytale", "minecraft_java", "minecraft_bedrock", "steam", "console_log", "generic_log"]),
    online: z.boolean(),
    first_seen_at: z.string().min(1),
    last_seen_at: z.string().min(1),
    connected_at: z.string().nullable(),
    disconnected_at: z.string().nullable(),
}).strict();
export const PlayerSnapshotSchema = z.object({
    instance_id: z.string().uuid(),
    online_count: z.number().int().nonnegative(),
    detection: z.enum(["console_log", "unavailable"]),
    access_mode: z.enum(["native_files", "console_commands", "shared_admin_password", "game_managed", "unsupported"]),
    players: z.array(ServerPlayerSchema).max(1_000),
}).strict();

export const InstalledModSchema = z.object({
    id: z.string().uuid(),
    instance_id: z.string().uuid(),
    source: z.string().min(1).max(64),
    display_name: z.string().min(1).max(255),
    checksum_sha256: z.string().regex(/^[0-9a-f]{64}$/i),
    size_bytes: z.number().int().positive().max(512 * 1_024 * 1_024),
    provider_project_id: z.string().nullable(),
    provider_version_id: z.string().nullable(),
    enabled: z.boolean(),
    created_at: z.string().min(1),
}).strict();

export const InstalledModListSchema = z.object({
    items: z.array(InstalledModSchema),
}).strict();

export const ModProviderConfigurationSchema = z.object({ configured: z.boolean() }).strict();
export const ModProviderStatusSchema = z.object({
    modrinth: ModProviderConfigurationSchema,
    curseforge: ModProviderConfigurationSchema,
}).strict();
export type ModProviderStatus = z.infer<typeof ModProviderStatusSchema>;

const safeRelativePath = z.string().min(1).max(512).refine((value) => {
    const normalized = value.replaceAll("\\", "/");
    return !value.startsWith("/")
        && !value.startsWith("\\")
        && !/^[A-Za-z]:/.test(value)
        && !value.includes(":")
        && !value.includes("\0")
        && ![...value].some((character) => (character.codePointAt(0) ?? 0) < 32)
        && !normalized.split("/").includes("..");
});

const optionalRelativePath = z.union([safeRelativePath, z.null()]);
const SteamProfileDefinitionBaseSchema = z.object({
    name: z.string().min(1).max(80).refine((value) => value.trim() === value && ![...value].some((character) => (character.codePointAt(0) ?? 0) < 32)),
    description: z.string().min(1).max(500).refine((value) => value.trim() === value && ![...value].some((character) => (character.codePointAt(0) ?? 0) < 32)),
    app_id: z.number().int().min(1).max(4_294_967_295),
    branch: z.string().regex(/^[A-Za-z0-9._-]{1,64}$/).nullable(),
    executable: z.object({
        linux_x86_64: optionalRelativePath,
        windows_x86_64: optionalRelativePath.refine((value) => value === null || value.toLowerCase().endsWith(".exe")),
    }).strict().refine((value) => value.linux_x86_64 !== null || value.windows_x86_64 !== null),
    arguments: z.array(z.string().max(8_192).refine((value) => ![...value].some((character) => {
        const code = character.codePointAt(0) ?? 0;
        return code < 32 && character !== "\t";
    }))).max(128),
    ports: z.array(PortSpecSchema.strict()).min(1).max(16),
    save_paths: z.array(safeRelativePath.refine((value) => !/[*?[\]]/.test(value))).min(1).max(32),
    ready_log_pattern: z.string().min(1).max(256).nullable(),
    stop_strategy: StopStrategySchema,
}).strict();

export const SteamProfileDefinitionSchema = SteamProfileDefinitionBaseSchema.superRefine((definition, context) => {
    const portNames = new Set<string>();
    const bindings = new Set<string>();
    definition.ports.forEach((port, index) => {
        if (!/^[a-z][a-z0-9_]{0,31}$/.test(port.name) || portNames.has(port.name)) {
            context.addIssue({ code: "custom", message: "invalid_port_name", path: ["ports", index, "name"] });
        }
        portNames.add(port.name);
        const binding = `${port.protocol}:${port.default}`;
        if (bindings.has(binding)) context.addIssue({ code: "custom", message: "duplicate_port", path: ["ports", index, "default"] });
        bindings.add(binding);
    });
    definition.ports.forEach((port, index) => {
        if (!port.adjacent_to) return;
        const parent = definition.ports.find((candidate) => candidate.name === port.adjacent_to);
        if (!parent || parent.name === port.name || parent.protocol !== port.protocol || parent.default + 1 !== port.default) {
            context.addIssue({ code: "custom", message: "invalid_adjacent_port", path: ["ports", index, "adjacent_to"] });
        }
    });
    definition.arguments.forEach((argument, index) => {
        if (argument === "{{instance_dir}}") return;
        const match = /^\{\{port:([a-z][a-z0-9_]{0,31})\}\}$/.exec(argument);
        if (match && portNames.has(match[1] ?? "")) return;
        if (argument.includes("{{") || argument.includes("}}")) {
            context.addIssue({ code: "custom", message: "invalid_placeholder", path: ["arguments", index] });
        }
    });
    if (new Set(definition.save_paths).size !== definition.save_paths.length) {
        context.addIssue({ code: "custom", message: "duplicate_save_path", path: ["save_paths"] });
    }
    if (definition.stop_strategy.timeout_seconds > 300) {
        context.addIssue({ code: "custom", message: "invalid_stop_timeout", path: ["stop_strategy", "timeout_seconds"] });
    }
    if (definition.stop_strategy.kind === "stdin" && (!definition.stop_strategy.command || definition.stop_strategy.command.length > 256 || /[\0\r\n]/.test(definition.stop_strategy.command))) {
        context.addIssue({ code: "custom", message: "invalid_stop_command", path: ["stop_strategy", "command"] });
    }
});

export const CreateSteamProfileSchema = z.object({
    id: z.string().regex(/^steam-(?!custom$)[a-z0-9]+(?:-[a-z0-9]+)*$/).max(64),
    definition: SteamProfileDefinitionSchema,
}).strict();
export const SteamProfileRevisionListSchema = z.array(GameProfileSchema);

export const ScheduleTriggerSchema = z.discriminatedUnion("kind", [
    z.object({ kind: z.literal("cron"), expression: z.string().min(1).max(455), timezone: z.string().min(1).max(64) }).strict(),
    z.object({ kind: z.literal("interval"), seconds: z.number().int().min(60).max(31_536_000) }).strict(),
]);
export const ScheduleActionSchema = z.discriminatedUnion("kind", [
    z.object({ kind: z.literal("start") }).strict(),
    z.object({ kind: z.literal("stop") }).strict(),
    z.object({ kind: z.literal("restart") }).strict(),
    z.object({ kind: z.literal("backup") }).strict(),
    z.object({ kind: z.literal("update") }).strict(),
    z.object({ kind: z.literal("console"), command: z.string().min(1).max(4_096).refine((value) => !/[\0\r\n]/.test(value)) }).strict(),
]);
export const ScheduleSchema = z.object({
    id: z.string().uuid(),
    instance_id: z.string().uuid(),
    name: z.string().min(1).max(80),
    trigger: ScheduleTriggerSchema,
    action: ScheduleActionSchema,
    enabled: z.boolean(),
    next_run_at: z.string().nullable(),
    last_run_at: z.string().nullable(),
    last_job_id: z.string().uuid().nullable(),
    version: z.number().int().positive(),
    created_by: z.string().uuid(),
    requested_by: z.string().uuid(),
    created_at: z.string().min(1),
    updated_at: z.string().min(1),
}).strict();
export const ScheduleListSchema = z.array(ScheduleSchema);
export const CreateScheduleSchema = z.object({
    instance_id: z.string().uuid(),
    name: z.string().trim().min(1).max(80),
    trigger: ScheduleTriggerSchema,
    action: ScheduleActionSchema,
    enabled: z.boolean(),
}).strict();
export const UpdateScheduleSchema = CreateScheduleSchema.omit({ instance_id: true }).strict();

export const HytaleDeviceAuthorizationSchema = z.object({
    job_id: z.string().uuid(),
    interaction: HytaleDeviceInteractionSchema,
}).strict();

export const BedrockArchiveAuthorizationSchema = z.object({
    job_id: z.string().uuid(),
    interaction: BedrockArchiveInteractionSchema,
}).strict();

export const DISCORD_WEBHOOK_EVENTS = [
    "backup.created",
    "backup.restored",
    "job.failed",
    "server.crashed",
    "server.started",
    "server.stopped",
    "server.update_applied",
    "server.update_failed",
    "server.update_rolled_back",
] as const;

export const DiscordWebhookEventSchema = z.enum(DISCORD_WEBHOOK_EVENTS);
export const DiscordWebhookSchema = z.object({
    id: z.string().uuid(),
    name: z.string().min(1).max(64),
    events: z.array(DiscordWebhookEventSchema).min(1).max(DISCORD_WEBHOOK_EVENTS.length),
    enabled: z.boolean(),
    configured: z.boolean(),
    version: z.number().int().positive(),
    last_delivery_at: z.string().nullable(),
    last_error_code: z.literal("delivery_failed").nullable(),
    created_at: z.string().min(1),
    updated_at: z.string().min(1),
}).strict();
export const DiscordWebhookListSchema = z.array(DiscordWebhookSchema);

const DiscordWebhookUrlSchema = z.string().min(1).max(2_048).refine((value) => {
    try {
        const url = new URL(value);
        const segments = url.pathname.split("/").filter(Boolean);
        return url.protocol === "https:"
            && url.hostname === "discord.com"
            && url.port === ""
            && url.username === ""
            && url.password === ""
            && url.search === ""
            && url.hash === ""
            && segments.length === 4
            && segments[0] === "api"
            && segments[1] === "webhooks"
            && /^\d+$/.test(segments[2] ?? "")
            && /^[A-Za-z0-9_-]{32,256}$/.test(segments[3] ?? "");
    } catch {
        return false;
    }
});

const DiscordWebhookBaseSchema = z.object({
    name: z.string().trim().min(1).max(64).refine((value) => !/\p{Cc}/u.test(value)),
    events: z.array(DiscordWebhookEventSchema).min(1).max(DISCORD_WEBHOOK_EVENTS.length)
        .refine((events) => new Set(events).size === events.length),
    enabled: z.boolean(),
}).strict();

export const CreateDiscordWebhookSchema = DiscordWebhookBaseSchema.extend({
    url: DiscordWebhookUrlSchema,
}).strict();
export const UpdateDiscordWebhookSchema = DiscordWebhookBaseSchema.extend({
    url: DiscordWebhookUrlSchema.optional(),
}).strict();

export type ActivitySummary = z.infer<typeof ActivitySummarySchema>;
export type ActivityJobsPage = z.infer<typeof ActivityJobsPageSchema>;
export type AuditEvent = z.infer<typeof AuditEventSchema>;
export type AuditPage = z.infer<typeof AuditPageSchema>;
export type NetworkSettings = z.infer<typeof NetworkSettingsSchema>;
export type ManagedFileEntry = z.infer<typeof ManagedFileEntrySchema>;
export type Backup = z.infer<typeof BackupSchema>;
export type MetricPeriod = z.infer<typeof MetricPeriodSchema>;
export type MetricPoint = z.infer<typeof MetricPointSchema>;
export type MetricsHistory = z.infer<typeof MetricsHistorySchema>;
export type CurrentServerMetric = z.infer<typeof CurrentServerMetricSchema>;
export type CurrentServerMetrics = z.infer<typeof CurrentServerMetricsSchema>;
export type SystemMetricsSnapshot = z.infer<typeof SystemMetricsSnapshotSchema>;
export type ConfigFileCategory = z.infer<typeof ConfigFileCategorySchema>;
export type ConfigFileSummary = z.infer<typeof ConfigFileSummarySchema>;
export type ConfigFileDocument = z.infer<typeof ConfigFileDocumentSchema>;
export type ServerPlayer = z.infer<typeof ServerPlayerSchema>;
export type PlayerSnapshot = z.infer<typeof PlayerSnapshotSchema>;
export type InstalledMod = z.infer<typeof InstalledModSchema>;
export type SteamProfileDefinition = z.infer<typeof SteamProfileDefinitionSchema>;
export type CreateSteamProfile = z.infer<typeof CreateSteamProfileSchema>;
export type Schedule = z.infer<typeof ScheduleSchema>;
export type ScheduleTrigger = z.infer<typeof ScheduleTriggerSchema>;
export type ScheduleAction = z.infer<typeof ScheduleActionSchema>;
export type CreateSchedule = z.infer<typeof CreateScheduleSchema>;
export type UpdateSchedule = z.infer<typeof UpdateScheduleSchema>;
export type HytaleDeviceAuthorization = z.infer<typeof HytaleDeviceAuthorizationSchema>;
export type BedrockArchiveAuthorization = z.infer<typeof BedrockArchiveAuthorizationSchema>;
export type DiscordWebhookEvent = z.infer<typeof DiscordWebhookEventSchema>;
export type DiscordWebhook = z.infer<typeof DiscordWebhookSchema>;
export type CreateDiscordWebhook = z.infer<typeof CreateDiscordWebhookSchema>;
export type UpdateDiscordWebhook = z.infer<typeof UpdateDiscordWebhookSchema>;
