import { afterEach, describe, expect, test } from "bun:test";
import {
    ActivitySummarySchema,
    AuditPageSchema,
    ConfigFileDocumentSchema,
    CreateDiscordWebhookSchema,
    CreateSteamProfileSchema,
    DiscordWebhookSchema,
    BedrockArchiveAuthorizationSchema,
    HytaleDeviceAuthorizationSchema,
    InstalledModSchema,
    PlayerSnapshotSchema,
    ScheduleActionSchema,
} from "../src/schemas/operations";
import { BackupsClient } from "../src/services/api/backups.client";
import { ActivityClient } from "../src/services/api/activity.client";
import { setCsrfToken } from "../src/services/api/base.client";
import { ConfigClient } from "../src/services/api/config.client";
import { FilesClient } from "../src/services/api/files.client";
import { ImportsClient } from "../src/services/api/imports.client";
import { JobsClient } from "../src/services/api/jobs.client";
import { PanelClient } from "../src/services/api/panel.client";
import { PlayersClient } from "../src/services/api/players.client";
import { ModsClient } from "../src/services/api/mods.client";
import { ProfileClient } from "../src/services/api/profile.client";
import { SchedulesClient } from "../src/services/api/schedules.client";
import { ServerClient } from "../src/services/api/server.client";
import { WebhooksClient } from "../src/services/api/webhooks.client";

const originalFetch = globalThis.fetch;
const originalXmlHttpRequest = globalThis.XMLHttpRequest;
const SERVER_ID = "11111111-1111-4111-8111-111111111111";
const BACKUP_ID = "22222222-2222-4222-8222-222222222222";

afterEach(() => {
    globalThis.fetch = originalFetch;
    globalThis.XMLHttpRequest = originalXmlHttpRequest;
    setCsrfToken(null);
});

describe("clients opérationnels", () => {
    test("valide strictement le résumé d’activité et le journal sans contenu sensible inattendu", () => {
        expect(ActivitySummarySchema.safeParse({
            active_jobs: 1,
            waiting_for_user: 0,
            failed_jobs_24h: 0,
            crashed_servers: 0,
            config_conflicts: 0,
        }).success).toBe(true);
        const payload = {
            items: [{
                id: 1,
                actor_user_id: BACKUP_ID,
                actor_username: "owner",
                action: "server.updated",
                resource_type: "instance",
                resource_id: SERVER_ID,
                outcome: "success",
                metadata: {},
                created_at: "2026-07-13T12:00:00Z",
                secret: "must-not-pass",
            }],
            next_before_id: null,
        };
        expect(AuditPageSchema.safeParse(payload).success).toBe(false);
    });

    test("pagine l’activité et filtre le journal avec des paramètres encodés", async () => {
        const inputs: string[] = [];
        globalThis.fetch = async (input) => {
            inputs.push(String(input));
            return Response.json({ items: [], ...(String(input).includes("/activity/jobs") ? { next_cursor: null } : { next_before_id: null }) });
        };

        await new ActivityClient().jobs({ cursor: BACKUP_ID, limit: 25, state: "failed", instance_id: SERVER_ID });
        await new ActivityClient().audit({ actor_user_id: BACKUP_ID, outcome: "failure", action: "server.updated" });

        expect(inputs[0]).toBe(`/api/v1/activity/jobs?cursor=${BACKUP_ID}&limit=25&state=failed&instance_id=${SERVER_ID}`);
        expect(inputs[1]).toBe(`/api/v1/audit?actor_user_id=${BACKUP_ID}&action=server.updated&outcome=failure`);
    });

    test("met à jour l’hôte global avec CSRF et concurrence explicite", async () => {
        let captured: RequestInit | undefined;
        globalThis.fetch = async (_input, init) => {
            captured = init;
            return Response.json({ advertised_game_host: "play.example.com", version: 4, updated_at: "2026-07-13T12:00:00Z" });
        };
        setCsrfToken("csrf-network");
        const response = await new PanelClient().updateNetwork("play.example.com", 3);
        expect(response.success).toBe(true);
        expect(captured?.method).toBe("PUT");
        expect(new Headers(captured?.headers).get("X-CSRF-Token")).toBe("csrf-network");
        expect(captured?.body).toBe(JSON.stringify({ advertised_game_host: "play.example.com", expected_version: 3 }));
    });

    test("relit l’historique borné du terminal d’installation", async () => {
        let input = "";
        globalThis.fetch = async (requestInput) => {
            input = String(requestInput);
            return Response.json({
                source: "install",
                items: [
                    { stream: "install", message: "Downloading..." },
                    { stream: "install_error", message: "Waiting for authentication" },
                ],
            });
        };

        const response = await new ServerClient().getLogHistory(SERVER_ID, "install");

        expect(response.success).toBe(true);
        expect(input).toBe(`/api/v1/servers/${SERVER_ID}/logs?source=install&limit=10000`);
        if (response.success) expect(response.data.items).toHaveLength(2);
    });

    test("met une configuration native en file avec son hash de concurrence", async () => {
        let input = "";
        let captured: RequestInit | undefined;
        const path = "game/Server/config.json";
        const payload = {
            file: {
                path,
                category: "configuration",
                format: "json",
                exists: true,
                size_bytes: 16,
                modified_at: "2026-07-17T12:00:00Z",
                sha256: "a".repeat(64),
                queued_change: {
                    id: BACKUP_ID,
                    status: "pending",
                    content_sha256: "b".repeat(64),
                    error_code: null,
                    queued_at: "2026-07-17T12:01:00Z",
                },
            },
            content: "{\"before\":true}",
            queued_content: "{\"after\":true}",
        };
        expect(ConfigFileDocumentSchema.safeParse(payload).success).toBe(true);
        globalThis.fetch = async (requestInput, init) => {
            input = String(requestInput);
            captured = init;
            return Response.json(payload);
        };
        setCsrfToken("csrf-config");

        const response = await new ConfigClient().queue(
            SERVER_ID,
            path,
            payload.queued_content,
            payload.file.sha256,
        );

        expect(response.success).toBe(true);
        expect(input).toBe(`/api/v1/servers/${SERVER_ID}/config-files/text?path=game%2FServer%2Fconfig.json`);
        expect(new Headers(captured?.headers).get("X-CSRF-Token")).toBe("csrf-config");
        expect(JSON.parse(String(captured?.body))).toEqual({
            content: payload.queued_content,
            expected_sha256: payload.file.sha256,
        });
    });

    test("valide une présence joueur normalisée sans accepter d’adresse réseau", async () => {
        const payload = {
            instance_id: SERVER_ID,
            online_count: 1,
            detection: "console_log",
            access_mode: "native_files",
            players: [{
                player_key: "id:ebc152b4-fdfd-4467-aadc-865ad4c87ea4",
                display_name: "TheFRcRaZy",
                external_id: "ebc152b4-fdfd-4467-aadc-865ad4c87ea4",
                source: "hytale",
                online: true,
                first_seen_at: "2026-07-17T12:00:00Z",
                last_seen_at: "2026-07-17T12:01:00Z",
                connected_at: "2026-07-17T12:01:00Z",
                disconnected_at: null,
            }],
        };
        expect(PlayerSnapshotSchema.safeParse(payload).success).toBe(true);
        expect(PlayerSnapshotSchema.safeParse({
            ...payload,
            players: [{ ...payload.players[0], ip_address: "192.0.2.1" }],
        }).success).toBe(false);
        globalThis.fetch = async () => Response.json(payload);

        const response = await new PlayersClient().snapshot(SERVER_ID);
        expect(response.success).toBe(true);
        if (response.success) expect(response.data.online_count).toBe(1);
    });

    test("envoie un upload brut et le CSRF via le client central", async () => {
        let input = "";
        let captured: RequestInit | undefined;
        globalThis.fetch = async (requestInput, init) => {
            input = String(requestInput);
            captured = init;
            return Response.json({ bytes_written: 3 }, { status: 201 });
        };
        setCsrfToken("csrf-files");

        const response = await new FilesClient().upload(SERVER_ID, "world/data.bin", new Blob(["abc"]));

        expect(response.success).toBe(true);
        expect(input).toBe(`/api/v1/files/content?instance_id=${SERVER_ID}&path=world%2Fdata.bin`);
        const headers = new Headers(captured?.headers);
        expect(headers.get("Content-Type")).toBe("application/octet-stream");
        expect(headers.get("X-CSRF-Token")).toBe("csrf-files");
        expect(headers.has("Authorization")).toBe(false);
    });

    test("utilise une clé d’idempotence pour les sauvegardes et encode les URLs de flux", async () => {
        let captured: RequestInit | undefined;
        globalThis.fetch = async (_input, init) => {
            captured = init;
            return Response.json({
                id: "33333333-3333-4333-8333-333333333333",
                instance_id: SERVER_ID,
                kind: "backup.create",
                state: "queued",
                progress: 0,
                requested_by: BACKUP_ID,
                created_at: "2026-07-13T12:00:00Z",
                interaction: null,
            }, { status: 202 });
        };

        const client = new BackupsClient();
        const response = await client.create(SERVER_ID, "operation-123");

        expect(response.success).toBe(true);
        expect(new Headers(captured?.headers).get("Idempotency-Key")).toBe("operation-123");
        expect(client.downloadUrl(BACKUP_ID)).toBe(`/api/v1/backups/${BACKUP_ID}/download`);
        expect(new FilesClient().downloadUrl(SERVER_ID, "world name/data.zip"))
            .toBe(`/api/v1/files/content?instance_id=${SERVER_ID}&path=world+name%2Fdata.zip`);
    });

    test("valide strictement les métadonnées d’un mod", () => {
        const mod = {
            id: BACKUP_ID,
            instance_id: SERVER_ID,
            source: "manual",
            display_name: "example.jar",
            checksum_sha256: "a".repeat(64),
            size_bytes: 4,
            provider_project_id: null,
            provider_version_id: null,
            enabled: true,
            created_at: "2026-07-13T12:00:00Z",
        };
        expect(InstalledModSchema.parse(mod)).toEqual(mod);
        expect(InstalledModSchema.safeParse({ ...mod, download_url: "https://evil.example/mod.jar" }).success).toBe(false);
    });

    test("importe un JAR par XHR avec progression, cookie et CSRF", async () => {
        const responseBody = {
            id: BACKUP_ID,
            instance_id: SERVER_ID,
            source: "manual",
            display_name: "my plugin.jar",
            checksum_sha256: "b".repeat(64),
            size_bytes: 4,
            provider_project_id: null,
            provider_version_id: null,
            enabled: true,
            created_at: "2026-07-13T12:00:00Z",
        };

        class FakeEventTarget {
            listeners = new Map<string, Array<(event: any) => void>>();
            addEventListener(type: string, listener: (event: any) => void) {
                this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
            }
            dispatch(type: string, event: any = {}) {
                for (const listener of this.listeners.get(type) ?? []) listener(event);
            }
        }
        class FakeXmlHttpRequest extends FakeEventTarget {
            static latest: FakeXmlHttpRequest | null = null;
            upload = new FakeEventTarget();
            headers = new Map<string, string>();
            method = "";
            url = "";
            body: Document | XMLHttpRequestBodyInit | null = null;
            status = 201;
            responseText = JSON.stringify(responseBody);
            withCredentials = false;
            constructor() {
                super();
                FakeXmlHttpRequest.latest = this;
            }
            open(method: string, url: string) { this.method = method; this.url = url; }
            setRequestHeader(name: string, value: string) { this.headers.set(name, value); }
            getResponseHeader() { return null; }
            send(body: Document | XMLHttpRequestBodyInit | null) {
                this.body = body;
                queueMicrotask(() => {
                    this.upload.dispatch("progress", { lengthComputable: true, loaded: 4, total: 4 });
                    this.dispatch("load");
                });
            }
            abort() { this.dispatch("abort"); }
        }
        globalThis.XMLHttpRequest = FakeXmlHttpRequest as unknown as typeof XMLHttpRequest;
        setCsrfToken("csrf-mods");
        const progress: number[] = [];
        const file = new File([new Uint8Array([0x50, 0x4b, 0x03, 0x04])], "my plugin.jar", { type: "application/java-archive" });

        const task = new ModsClient().uploadManual(SERVER_ID, file, (value) => progress.push(value.percent));
        const response = await task.response;
        const xhr = FakeXmlHttpRequest.latest;

        expect(response.success).toBe(true);
        expect(xhr?.method).toBe("POST");
        expect(xhr?.url).toBe(`/api/v1/servers/${SERVER_ID}/mods/manual?filename=my+plugin.jar`);
        expect(xhr?.headers.get("Content-Type")).toBe("application/java-archive");
        expect(xhr?.headers.get("X-CSRF-Token")).toBe("csrf-mods");
        expect(xhr?.headers.has("Authorization")).toBe(false);
        expect(xhr?.withCredentials).toBe(true);
        expect(xhr?.body).toBe(file);
        expect(progress.at(-1)).toBe(100);
    });

    test("envoie un ZIP Bedrock brut avec SHA-256, idempotence, CSRF et progression", async () => {
        const responseBody = {
            id: BACKUP_ID,
            instance_id: SERVER_ID,
            kind: "install",
            state: "queued",
            progress: 0,
            requested_by: BACKUP_ID,
            created_at: "2026-07-13T12:00:00Z",
            interaction: null,
        };
        class FakeEventTarget {
            listeners = new Map<string, Array<(event: any) => void>>();
            addEventListener(type: string, listener: (event: any) => void) {
                this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
            }
            dispatch(type: string, event: any = {}) {
                for (const listener of this.listeners.get(type) ?? []) listener(event);
            }
        }
        class FakeXmlHttpRequest extends FakeEventTarget {
            static latest: FakeXmlHttpRequest | null = null;
            upload = new FakeEventTarget();
            headers = new Map<string, string>();
            method = "";
            url = "";
            body: Document | XMLHttpRequestBodyInit | null = null;
            status = 202;
            responseText = JSON.stringify(responseBody);
            withCredentials = false;
            constructor() { super(); FakeXmlHttpRequest.latest = this; }
            open(method: string, url: string) { this.method = method; this.url = url; }
            setRequestHeader(name: string, value: string) { this.headers.set(name, value); }
            getResponseHeader() { return null; }
            send(body: Document | XMLHttpRequestBodyInit | null) {
                this.body = body;
                queueMicrotask(() => {
                    this.upload.dispatch("progress", { lengthComputable: true, loaded: 4, total: 4 });
                    this.dispatch("load");
                });
            }
            abort() { this.dispatch("abort"); }
        }
        globalThis.XMLHttpRequest = FakeXmlHttpRequest as unknown as typeof XMLHttpRequest;
        setCsrfToken("csrf-import");
        const digest = "a".repeat(64);
        const progress: number[] = [];
        const file = new File([new Uint8Array([0x50, 0x4b, 0x03, 0x04])], "bedrock.zip", { type: "application/zip" });

        const task = new ImportsClient().uploadZip(SERVER_ID, file, {
            idempotencyKey: BACKUP_ID,
            sha256: digest,
            onProgress: (value) => progress.push(value.percent),
        });
        const response = await task.response;
        const xhr = FakeXmlHttpRequest.latest;

        expect(response.success).toBe(true);
        expect(xhr?.method).toBe("POST");
        expect(xhr?.url).toBe(`/api/v1/servers/${SERVER_ID}/imports/zip`);
        expect(xhr?.headers.get("Content-Type")).toBe("application/zip");
        expect(xhr?.headers.get("Idempotency-Key")).toBe(BACKUP_ID);
        expect(xhr?.headers.get("X-Dmx-Archive-Sha256")).toBe(digest);
        expect(xhr?.headers.get("X-CSRF-Token")).toBe("csrf-import");
        expect(xhr?.withCredentials).toBe(true);
        expect(xhr?.body).toBe(file);
        expect(progress.at(-1)).toBe(100);
    });

    test("persiste l’interaction Bedrock typée et rejette tout champ secret arbitraire", () => {
        const payload = {
            job_id: BACKUP_ID,
            interaction: {
                kind: "bedrock_archive_upload",
                instance_id: SERVER_ID,
                version: "1.21.0",
                method: "POST",
                path: `/api/v1/servers/${SERVER_ID}/imports/zip`,
                required_sha256_header: "x-dmx-archive-sha256",
                max_bytes: 4 * 1024 * 1024 * 1024,
            },
        };
        expect(BedrockArchiveAuthorizationSchema.safeParse(payload).success).toBe(true);
        expect(BedrockArchiveAuthorizationSchema.safeParse({
            ...payload,
            interaction: { ...payload.interaction, secret: "must-not-pass" },
        }).success).toBe(false);
    });

    test("liste, annule et crée un import source avec les contrats jobs", async () => {
        const calls: Array<{ input: string; init?: RequestInit }> = [];
        const job = {
            id: BACKUP_ID,
            instance_id: SERVER_ID,
            kind: "install",
            state: "queued",
            progress: 0,
            requested_by: BACKUP_ID,
            created_at: "2026-07-13T12:00:00Z",
            interaction: null,
        };
        globalThis.fetch = async (input, init) => {
            calls.push({ input: String(input), init });
            return Response.json(String(input).endsWith("/jobs") ? [job] : job, { status: init?.method === "POST" ? 202 : 200 });
        };

        expect((await new JobsClient().list()).success).toBe(true);
        expect((await new JobsClient().cancel(BACKUP_ID)).success).toBe(true);
        expect((await new ImportsClient().copy(SERVER_ID, "/imports/server", BACKUP_ID)).success).toBe(true);

        expect(calls.map((call) => call.input)).toEqual([
            "/api/v1/jobs",
            `/api/v1/jobs/${BACKUP_ID}/cancel`,
            `/api/v1/servers/${SERVER_ID}/imports/copy`,
        ]);
        const importHeaders = new Headers(calls[2]?.init?.headers);
        expect(importHeaders.get("Idempotency-Key")).toBe(BACKUP_ID);
        expect(calls[2]?.init?.body).toBe(JSON.stringify({ source_path: "/imports/server" }));
    });

    test("valide les profils Steam sans shell, chemin hôte ni placeholder libre", () => {
        const valid = {
            id: "steam-example",
            definition: {
                name: "Example server",
                description: "Anonymous native depot",
                app_id: 123_456,
                branch: "public",
                executable: { linux_x86_64: "bin/server", windows_x86_64: "Server.exe" },
                arguments: ["--port", "{{port:game}}", "{{instance_dir}}"],
                ports: [{ name: "game", protocol: "udp", default: 27_015, adjacent_to: null }],
                save_paths: ["saves/world"],
                ready_log_pattern: "Ready",
                stop_strategy: { kind: "interrupt", timeout_seconds: 60 },
            },
        };
        expect(CreateSteamProfileSchema.safeParse(valid).success).toBe(true);
        expect(CreateSteamProfileSchema.safeParse({ ...valid, definition: { ...valid.definition, executable: { linux_x86_64: "/usr/bin/sh", windows_x86_64: null } } }).success).toBe(false);
        expect(CreateSteamProfileSchema.safeParse({ ...valid, definition: { ...valid.definition, arguments: ["{{env:PATH}}"] } }).success).toBe(false);
        expect(CreateSteamProfileSchema.safeParse({ ...valid, definition: { ...valid.definition, save_paths: ["../outside"] } }).success).toBe(false);
    });

    test("envoie la révision Steam avec If-Match et garde AppID hors des instances", async () => {
        let input = "";
        let captured: RequestInit | undefined;
        const profile = {
            id: "steam-example", revision: 3, name: "Example", description: "Server", kind: "steam_custom",
            platforms: ["linux-x64"], capabilities: ["install"],
            ports: [{ name: "game", protocol: "udp", default: 27_015, adjacent_to: null }],
            lifecycle: { stop: { kind: "interrupt", timeout_seconds: 60 }, ready_log_pattern: "Ready" },
            settings_schema: { type: "object", additionalProperties: false, required: [], properties: {} }, ui_schema: {},
            steam_profile: {
                app_id: 123_456, branch: null, executable: { linux_x86_64: "server", windows_x86_64: null }, arguments: [],
                ports: [{ name: "game", protocol: "udp", default: 27_015, adjacent_to: null }], save_paths: ["saves"],
                ready_log_pattern: "Ready", stop_strategy: { kind: "interrupt", timeout_seconds: 60 },
            },
        };
        globalThis.fetch = async (requestInput, init) => {
            input = String(requestInput);
            captured = init;
            return Response.json(profile, { status: 201 });
        };

        const response = await new ProfileClient().reviseSteam("steam-example", {
            name: "Example",
            description: "Server",
            app_id: 123_456,
            branch: null,
            executable: { linux_x86_64: "server", windows_x86_64: null },
            arguments: [],
            ports: [{ name: "game", protocol: "udp", default: 27_015, adjacent_to: null }],
            save_paths: ["saves"],
            ready_log_pattern: "Ready",
            stop_strategy: { kind: "interrupt", timeout_seconds: 60 },
        }, 2);

        expect(response.success).toBe(true);
        expect(input).toBe("/api/v1/game-profiles/steam/steam-example");
        expect(new Headers(captured?.headers).get("If-Match")).toBe('"2"');
        const body = JSON.parse(String(captured?.body));
        expect(body.app_id).toBe(123_456);
        expect(body.instance_id).toBeUndefined();
    });

    test("limite les tâches aux actions fermées et transmet leur ETag", async () => {
        expect(ScheduleActionSchema.safeParse({ kind: "backup" }).success).toBe(true);
        expect(ScheduleActionSchema.safeParse({ kind: "script", command: "rm -rf /" }).success).toBe(false);
        let captured: RequestInit | undefined;
        globalThis.fetch = async (_input, init) => {
            captured = init;
            return Response.json({
                id: "44444444-4444-4444-8444-444444444444", instance_id: SERVER_ID, name: "Hourly backup",
                trigger: { kind: "interval", seconds: 3600 }, action: { kind: "backup" }, enabled: true,
                next_run_at: "2026-07-13T13:00:00Z", last_run_at: null, last_job_id: null, version: 5,
                created_by: BACKUP_ID, requested_by: BACKUP_ID, created_at: "2026-07-13T12:00:00Z", updated_at: "2026-07-13T12:00:00Z",
            });
        };
        const response = await new SchedulesClient().update("44444444-4444-4444-8444-444444444444", {
            name: "Hourly backup", trigger: { kind: "interval", seconds: 3600 }, action: { kind: "backup" }, enabled: true,
        }, 4);
        expect(response.success).toBe(true);
        expect(new Headers(captured?.headers).get("If-Match")).toBe('"4"');
    });

    test("n’accepte l’interaction OAuth appareil que depuis Hytale officiel", () => {
        const payload = {
            job_id: "55555555-5555-4555-8555-555555555555",
            interaction: { kind: "oauth_device", verification_uri: "https://oauth.accounts.hytale.com/oauth2/device/verify?user_code=x6nimECK", user_code: "x6nimECK" },
        };
        expect(HytaleDeviceAuthorizationSchema.safeParse(payload).success).toBe(true);
        expect(HytaleDeviceAuthorizationSchema.safeParse({
            ...payload,
            interaction: { ...payload.interaction, user_code: "EXPIRED1" },
        }).success).toBe(false);
        expect(HytaleDeviceAuthorizationSchema.safeParse({ ...payload, interaction: { ...payload.interaction, verification_uri: "https://accounts.hytale.com.evil.example/device" } }).success).toBe(false);
        expect(HytaleDeviceAuthorizationSchema.safeParse({ ...payload, interaction: { ...payload.interaction, verification_uri: "https://accounts.hytale.com/other" } }).success).toBe(false);
        expect(HytaleDeviceAuthorizationSchema.safeParse({ ...payload, interaction: { ...payload.interaction, verification_uri: "https://oauth.accounts.hytale.com/oauth2/device/verify?redirect=https://evil.example" } }).success).toBe(false);
        expect(HytaleDeviceAuthorizationSchema.safeParse({ ...payload, token: "must-not-pass" }).success).toBe(false);
    });

    test("limite les webhooks aux URL Discord officielles et aux événements fermés", () => {
        const token = "abcdefghijklmnopqrstuvwxyzABCDEF0123456789";
        const valid = {
            name: "Incidents production",
            url: `https://discord.com/api/webhooks/123456789012345678/${token}`,
            events: ["job.failed", "server.crashed"],
            enabled: true,
        };

        expect(CreateDiscordWebhookSchema.safeParse(valid).success).toBe(true);
        expect(CreateDiscordWebhookSchema.safeParse({ ...valid, url: valid.url.replace("https:", "http:") }).success).toBe(false);
        expect(CreateDiscordWebhookSchema.safeParse({ ...valid, url: valid.url.replace("discord.com", "discord.com.evil.example") }).success).toBe(false);
        expect(CreateDiscordWebhookSchema.safeParse({ ...valid, url: `${valid.url}?redirect=https://evil.example` }).success).toBe(false);
        expect(CreateDiscordWebhookSchema.safeParse({ ...valid, events: ["server.started", "server.command"] }).success).toBe(false);
        expect(CreateDiscordWebhookSchema.safeParse({ ...valid, events: ["job.failed", "job.failed"] }).success).toBe(false);
        expect(CreateDiscordWebhookSchema.safeParse({ ...valid, name: "Incidents\u007fproduction" }).success).toBe(false);
    });

    test("rejette toute URL secrète renvoyée par l’API webhook", () => {
        const webhook = {
            id: "66666666-6666-4666-8666-666666666666",
            name: "Incidents",
            events: ["job.failed"],
            enabled: true,
            configured: true,
            version: 2,
            last_delivery_at: null,
            last_error_code: null,
            created_at: "2026-07-13T12:00:00Z",
            updated_at: "2026-07-13T12:00:00Z",
        };

        expect(DiscordWebhookSchema.safeParse(webhook).success).toBe(true);
        expect(DiscordWebhookSchema.safeParse({
            ...webhook,
            url: "https://discord.com/api/webhooks/123/abcdefghijklmnopqrstuvwxyzABCDEF0123456789",
        }).success).toBe(false);
        expect(DiscordWebhookSchema.safeParse({ ...webhook, last_error_code: "discord_response_body" }).success).toBe(false);
    });

    test("met à jour un webhook avec If-Match sans retransmettre son URL", async () => {
        let input = "";
        let captured: RequestInit | undefined;
        const webhook = DiscordWebhookSchema.parse({
            id: "66666666-6666-4666-8666-666666666666",
            name: "Incidents",
            events: ["job.failed"],
            enabled: false,
            configured: true,
            version: 3,
            last_delivery_at: null,
            last_error_code: null,
            created_at: "2026-07-13T12:00:00Z",
            updated_at: "2026-07-13T12:05:00Z",
        });
        globalThis.fetch = async (requestInput, init) => {
            input = String(requestInput);
            captured = init;
            return Response.json(webhook);
        };

        const response = await new WebhooksClient().update(webhook.id, {
            name: webhook.name,
            events: webhook.events,
            enabled: false,
        }, 2);

        expect(response.success).toBe(true);
        expect(input).toBe(`/api/v1/webhooks/${webhook.id}`);
        expect(new Headers(captured?.headers).get("If-Match")).toBe('"2"');
        expect(JSON.parse(String(captured?.body))).toEqual({
            name: "Incidents",
            events: ["job.failed"],
            enabled: false,
        });
    });

    test("accepte la suppression webhook vide en 204", async () => {
        globalThis.fetch = async () => new Response(null, { status: 204 });

        const response = await new WebhooksClient().delete("66666666-6666-4666-8666-666666666666");

        expect(response.success).toBe(true);
        if (response.success) expect(response.data).toBeUndefined();
    });
});
