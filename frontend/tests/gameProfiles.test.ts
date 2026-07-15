import { describe, expect, test } from "bun:test";
import { GameProfileSchema, InstanceSchema } from "../src/schemas/api";
import { initialProfileSettings, isSafeRelativeExecutable, partitionProfileValues } from "../src/utils/profileSettings";

const valheimProfile = GameProfileSchema.parse({
    id: "valheim",
    revision: 1,
    name: "Valheim",
    description: "Serveur Valheim SteamCMD anonyme.",
    kind: "builtin",
    platforms: ["linux-x64", "windows-x64"],
    capabilities: ["console", "backups", "steamcmd"],
    ports: [
        { name: "port", protocol: "udp", default: 2456, adjacent_to: null },
        { name: "query_port", protocol: "udp", default: 2457, adjacent_to: "port" },
    ],
    lifecycle: { stop: { kind: "interrupt", timeout_seconds: 60 }, ready_log_pattern: "Game server connected" },
    settings_schema: {
        type: "object",
        additionalProperties: false,
        required: ["server_name", "world_name", "server_password"],
        properties: {
            server_name: { type: "string", minLength: 1, maxLength: 64 },
            world_name: { type: "string", minLength: 1, maxLength: 64 },
            port: { type: "integer", default: 2456, minimum: 1, maximum: 65534 },
            query_port: { type: "integer", default: 2457, minimum: 2, maximum: 65535 },
            server_password: { type: "string", secret: true, writeOnly: true, minLength: 5 },
        },
    },
    ui_schema: { layout: "sections" },
});

describe("contrats des profils", () => {
    test("valide le manifeste complet renvoyé par le backend", () => {
        expect(valheimProfile.ports[1]?.adjacent_to).toBe("port");
        expect(valheimProfile.lifecycle.stop.kind).toBe("interrupt");
    });

    test("refuse un ancien profil sans kind, ports et lifecycle", () => {
        const legacy = { ...valheimProfile } as Record<string, unknown>;
        delete legacy.kind;
        delete legacy.ports;
        delete legacy.lifecycle;
        expect(GameProfileSchema.safeParse(legacy).success).toBe(false);
    });

    test("sépare les secrets des réglages persistés", () => {
        const initial = initialProfileSettings(valheimProfile);
        const values = { ...initial, server_name: "DMX", world_name: "World", server_password: "not-in-settings" };
        const partitioned = partitionProfileValues(valheimProfile, values);
        expect(partitioned.settings.server_password).toBeUndefined();
        expect(partitioned.secrets.server_password).toBe("not-in-settings");
    });

    test("refuse les exécutables absolus et les traversals", () => {
        expect(isSafeRelativeExecutable("server/bin/start_server")).toBe(true);
        expect(isSafeRelativeExecutable("../outside/server")).toBe(false);
        expect(isSafeRelativeExecutable("C:\\server\\start.exe")).toBe(false);
        expect(isSafeRelativeExecutable("/usr/bin/server")).toBe(false);
        expect(isSafeRelativeExecutable("server.exe:alternate-stream")).toBe(false);
        expect(isSafeRelativeExecutable("server\nname")).toBe(false);
    });
});

describe("contrat Instance", () => {
    test("accepte uniquement le DTO versionné du backend", () => {
        const instance = {
            id: "de305d54-75b4-431b-adb2-eb6b9e546014",
            name: "Valheim principal",
            profile_id: "valheim",
            profile_revision: 1,
            settings: { port: 2456, query_port: 2457 },
            config_version: 3,
            installation_state: "installed",
            installed_version: "0.219.16",
            installed_build: null,
            desired_state: "stopped",
            runtime_state: "stopped",
            managed: true,
            auto_start: false,
            watchdog_enabled: true,
            created_at: "2026-07-13T10:00:00Z",
            updated_at: "2026-07-13T10:00:00Z",
        };
        expect(InstanceSchema.parse(instance).config_version).toBe(3);
        expect(InstanceSchema.safeParse({ ...instance, runtime_state: undefined, status: "running" }).success).toBe(false);
    });
});
