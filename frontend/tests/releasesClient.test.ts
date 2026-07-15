import { afterEach, describe, expect, test } from "bun:test";
import { PanelReleaseStatusSchema } from "../src/schemas/releases";
import { setCsrfToken } from "../src/services/api/base.client";
import { ReleasesClient } from "../src/services/api/releases.client";

const originalFetch = globalThis.fetch;

const status = {
    configured: true,
    current_version: "1.0.0",
    deployment_mode: "docker",
    state: "update_available",
    checked_at: "2026-07-13T12:00:00Z",
    latest: {
        version: "1.0.1",
        published_at: "2026-07-13T11:00:00Z",
        notes_url: "https://github.com/thefrcrazy/DmxServerManager/releases/tag/v1.0.1",
        target: {
            kind: "docker",
            image: "ghcr.io/thefrcrazy/dmx-server-manager",
            digest: `sha256:${"a".repeat(64)}`,
            pull_command: `docker pull 'ghcr.io/thefrcrazy/dmx-server-manager@sha256:${"a".repeat(64)}'`,
            apply_command: `DMX_IMAGE='ghcr.io/thefrcrazy/dmx-server-manager@sha256:${"a".repeat(64)}' docker compose up -d --force-recreate panel`,
        },
    },
    error_code: null,
} as const;

afterEach(() => {
    globalThis.fetch = originalFetch;
    setCsrfToken(null);
});

describe("détection de release signée", () => {
    test("valide uniquement l’image officielle et un digest SHA-256 épinglé", () => {
        expect(PanelReleaseStatusSchema.safeParse(status).success).toBe(true);
        expect(PanelReleaseStatusSchema.safeParse({
            ...status,
            latest: { ...status.latest, target: { ...status.latest.target, image: "evil.example/panel" } },
        }).success).toBe(false);
        expect(PanelReleaseStatusSchema.safeParse({
            ...status,
            latest: { ...status.latest, target: { ...status.latest.target, digest: "sha256:latest" } },
        }).success).toBe(false);
        expect(PanelReleaseStatusSchema.safeParse({ ...status, public_key: "must-not-leak" }).success).toBe(false);
    });

    test("la vérification manuelle utilise cookie et CSRF sans exécuter de commande", async () => {
        let input = "";
        let captured: RequestInit | undefined;
        globalThis.fetch = async (requestInput, init) => {
            input = String(requestInput);
            captured = init;
            return Response.json(status);
        };
        setCsrfToken("csrf-release");

        const response = await new ReleasesClient().check();

        expect(response.success).toBe(true);
        expect(input).toBe("/api/v1/releases/panel/check");
        expect(captured?.method).toBe("POST");
        const headers = new Headers(captured?.headers);
        expect(headers.get("X-CSRF-Token")).toBe("csrf-release");
        expect(headers.has("Authorization")).toBe(false);
        expect(captured?.body).toBeUndefined();
    });
});
