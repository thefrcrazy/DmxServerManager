// Zod schemas for runtime API response validation
import { z } from "zod";

// ============= Base Schemas =============

export const ApiErrorSchema = z.object({
    error: z.string(),
    code: z.string().optional(),
    details: z.unknown().optional(),
    debug: z.object({
        server_id: z.string().optional(),
        file_path: z.string().optional(),
    }).optional(),
});

export type ApiError = z.infer<typeof ApiErrorSchema>;

// Generic API response wrapper
export const createApiResponseSchema = <T extends z.ZodTypeAny>(dataSchema: T) =>
    z.object({
        data: dataSchema,
        success: z.boolean(),
        timestamp: z.string(),
    });

// ============= Auth Schemas =============

export const UserInfoSchema = z.object({
    id: z.string(),
    username: z.string(),
    role: z.string(),
    permissions: z.array(z.string()),
    accent_color: z.string().optional(),
});

export const AuthResponseSchema = z.object({
    token: z.string(),
    user: UserInfoSchema,
});

export const SetupStatusSchema = z.object({
    needs_setup: z.boolean(),
});

export type UserInfo = z.infer<typeof UserInfoSchema>;
export type AuthResponse = z.infer<typeof AuthResponseSchema>;
export type SetupStatus = z.infer<typeof SetupStatusSchema>;

// ============= Server Schemas =============

export const PlayerSchema = z.object({
    name: z.string(),
    is_online: z.boolean(),
    last_seen: z.string(),
    is_op: z.boolean().optional(),
    is_banned: z.boolean().optional(),
    is_whitelisted: z.boolean().optional(),
});

export const ServerSchema = z.object({
    id: z.string(),
    name: z.string(),
    game_type: z.string(),
    status: z.string().default("stopped"),
    executable_path: z.string(),
    working_dir: z.string(),
    java_path: z.string().nullable().optional(),
    min_memory: z.string().nullable().optional(),
    max_memory: z.string().nullable().optional(),
    extra_args: z.string().nullable().optional(),
    config: z.record(z.string(), z.unknown()).nullable().optional(),
    auto_start: z.boolean(),
    created_at: z.string().optional(),
    updated_at: z.string().optional(),
    dir_exists: z.boolean().default(true),
    players: z.array(PlayerSchema).nullable().optional(),
    max_players: z.number().nullable().optional(),
    port: z.number().optional(),
    bind_address: z.string().optional(),

    // Backup settings
    backup_enabled: z.boolean().optional(),
    backup_frequency: z.number().optional(),
    backup_max_backups: z.number().optional(),
    backup_prefix: z.string().optional(),

    // Discord settings
    discord_username: z.string().nullable().optional(),
    discord_avatar: z.string().nullable().optional(),
    discord_webhook_url: z.string().nullable().optional(),
    discord_notifications: z.record(z.string(), z.boolean()).nullable().optional(),

    // Other settings
    logs_retention_days: z.number().optional(),
    watchdog_enabled: z.boolean().optional(),
    auth_mode: z.string().optional(),

    // Runtime metrics - with defaults for compatibility
    cpu_usage: z.number().default(0),
    cpu_usage_normalized: z.number().default(0),
    memory_usage_bytes: z.number().default(0),
    max_memory_bytes: z.number().default(0),
    max_heap_bytes: z.number().default(0),
    disk_usage_bytes: z.number().default(0),
    started_at: z.string().nullable().optional(),
});

export type Server = z.infer<typeof ServerSchema>;
export type Player = z.infer<typeof PlayerSchema>;

// ============= System Schemas =============

export const SystemStatsSchema = z.object({
    cpu: z.number(),
    ram: z.number(),
    ram_used: z.number(),
    ram_total: z.number(),
    disk: z.number(),
    disk_used: z.number(),
    disk_total: z.number(),
    players_current: z.number(),
    players_max: z.number(),
    cpu_cores: z.number(),
    managed_cpu: z.number(),
    managed_cpu_normalized: z.number(),
    managed_ram: z.number(),
    managed_disk: z.number(),
});

export const JavaVersionSchema = z.object({
    path: z.string(),
    version: z.string(),
});

export type SystemStats = z.infer<typeof SystemStatsSchema>;
export type JavaVersion = z.infer<typeof JavaVersionSchema>;

// ============= Backup Schemas =============

export const BackupSchema = z.object({
    id: z.string(),
    server_id: z.string(),
    filename: z.string(),
    size_bytes: z.number(),
    created_at: z.string(),
});

export type Backup = z.infer<typeof BackupSchema>;

// ============= Settings Schemas =============

export const AppSettingsSchema = z.object({
    version: z.string().optional(),
    servers_dir: z.string().optional(),
    backups_dir: z.string().optional(),
    webhook_url: z.string().optional(),
});

export type AppSettings = z.infer<typeof AppSettingsSchema>;

// ============= Validation Helper =============

export function validateApiResponse<T>(schema: z.ZodType<T>, data: unknown): T {
    const result = schema.safeParse(data);
    if (!result.success) {
        console.warn("API response validation failed:", result.error.issues);
        // In development, throw; in production, return data as-is with warning
        if (import.meta.env.DEV) {
            throw new Error(`Validation failed: ${JSON.stringify(result.error.issues)}`);
        }
    }
    return data as T;
}

// ============= Metrics Schemas =============

export const MetricDataPointSchema = z.object({
    id: z.string(),
    server_id: z.string(),
    cpu_usage: z.number(),
    memory_bytes: z.number(),
    disk_bytes: z.number(),
    player_count: z.number(),
    recorded_at: z.string(),
});

export const MetricsHistoryResponseSchema = z.object({
    server_id: z.string(),
    period: z.string(),
    data: z.array(MetricDataPointSchema),
});

export type MetricDataPoint = z.infer<typeof MetricDataPointSchema>;
export type MetricsHistoryResponse = z.infer<typeof MetricsHistoryResponseSchema>;

