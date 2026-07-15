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
export const ChatMessageSchema = z.object({
    id: z.string().uuid(),
    author_user_id: z.string().uuid().nullable(),
    author_username: z.string().nullable(),
    body: z.string().nullable(),
    created_at: z.string().min(1),
    deleted_at: z.string().nullable(),
}).strict();

export const ChatPageSchema = z.object({
    items: z.array(ChatMessageSchema),
    next_before_id: z.string().uuid().nullable(),
}).strict();

const chatBody = z.string()
    .transform((value) => value.trim())
    .pipe(z.string().min(1).max(4_000))
    .refine((value) => new TextEncoder().encode(value).byteLength <= 16 * 1_024)
    .refine((value) => ![...value].some((character) => {
        const code = character.codePointAt(0) ?? 0;
        return code < 32 && character !== "\n" && character !== "\t";
    }));

export const ChatDraftSchema = z.object({ body: chatBody }).strict();
export const OperationSuccessSchema = z.object({
    success: z.boolean(),
    message: z.string().optional(),
}).strict();
export const AcceptedJobSchema = JobSchema.strict();
export const ChatDeletedEventSchema = z.object({
    id: z.string().uuid(),
    deleted_at: z.string().min(1),
}).strict();

export const NotificationSchema = z.object({
    id: z.string().uuid(),
    kind: z.string().min(1).max(64),
    message_key: z.string().min(1).max(128),
    data: z.record(z.string(), z.unknown()),
    read_at: z.string().nullable(),
    created_at: z.string().min(1),
}).strict();

export const NotificationPageSchema = z.object({
    items: z.array(NotificationSchema),
    next_before_id: z.string().uuid().nullable(),
    unread_count: z.number().int().nonnegative(),
}).strict();
export const NotificationReadEventSchema = z.object({
    id: z.string().uuid(),
    read_at: z.string().min(1),
}).strict();
export const NotificationsReadAllEventSchema = z.object({ read_at: z.string().min(1) }).strict();

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
    recorded_at: z.string().min(1),
}).strict();
export const MetricsHistorySchema = z.object({
    server_id: z.string().uuid(),
    period: MetricPeriodSchema,
    points: z.array(MetricPointSchema).max(10_000),
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

export type ChatMessage = z.infer<typeof ChatMessageSchema>;
export type ChatPage = z.infer<typeof ChatPageSchema>;
export type Notification = z.infer<typeof NotificationSchema>;
export type NotificationPage = z.infer<typeof NotificationPageSchema>;
export type ManagedFileEntry = z.infer<typeof ManagedFileEntrySchema>;
export type Backup = z.infer<typeof BackupSchema>;
export type MetricPeriod = z.infer<typeof MetricPeriodSchema>;
export type MetricPoint = z.infer<typeof MetricPointSchema>;
export type MetricsHistory = z.infer<typeof MetricsHistorySchema>;
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
