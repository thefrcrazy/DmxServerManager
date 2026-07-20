import { PermissionDescription } from "../schemas/api";

const PERMISSION_IDS = [
    "job.read",
    "mods.manage",
    "profile.manage",
    "profile.read",
    "schedule.manage",
    "server.backup",
    "server.backup.read",
    "server.config.raw.read",
    "server.config.raw.write",
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
] as const;

const INSTANCE_SCOPED = new Set<string>([
    "job.read",
    "mods.manage",
    "schedule.manage",
    "server.backup",
    "server.backup.read",
    "server.config.raw.read",
    "server.config.raw.write",
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

const HIGH_RISK = new Set<string>([
    "profile.manage",
    "server.config.raw.read",
    "server.config.raw.write",
    "server.console.write",
    "server.files.write",
]);

export const PERMISSION_CATALOG: PermissionDescription[] = PERMISSION_IDS.map((id) => ({
    id,
    high_risk: HIGH_RISK.has(id),
    instance_scoped: INSTANCE_SCOPED.has(id),
}));
