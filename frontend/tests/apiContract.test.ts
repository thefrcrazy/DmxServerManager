import { afterEach, describe, expect, test } from "bun:test";
import { z } from "zod";
import { BaseClient } from "../src/services/api/base.client";
import { ServerClient } from "../src/services/api/server.client";

const originalFetch = globalThis.fetch;

class TestClient extends BaseClient {
    getNumber() { return this.request("/value", z.object({ value: z.number() })); }
}

afterEach(() => { globalThis.fetch = originalFetch; });

describe("validation des réponses", () => {
    test("rejette une réponse 2xx qui ne respecte pas le schéma", async () => {
        globalThis.fetch = async () => Response.json({ value: "not-a-number" });
        const response = await new TestClient().getNumber();
        expect(response.success).toBe(false);
        if (!response.success) expect(response.error.message).toContain("Réponse API invalide");
    });

    test("expose le statut, le code et la trace application/problem+json", async () => {
        globalThis.fetch = async () => new Response(JSON.stringify({
            type: "about:blank",
            title: "servers.version_conflict",
            status: 409,
            code: "SRV_009",
            trace_id: "trace-1",
        }), { status: 409, headers: { "Content-Type": "application/problem+json" } });
        const response = await new TestClient().getNumber();
        expect(response.success).toBe(false);
        if (!response.success) {
            expect(response.error.status).toBe(409);
            expect(response.error.code).toBe("SRV_009");
            expect(response.error.traceId).toBe("trace-1");
        }
    });

    test("envoie If-Match avec la version de configuration", async () => {
        let headers = new Headers();
        const instance = {
            id: "de305d54-75b4-431b-adb2-eb6b9e546014", name: "Test", profile_id: "hytale", profile_revision: 1,
            settings: {}, config_version: 8, installation_state: "installed", installed_version: "1.0.0", installed_build: null, desired_state: "stopped", runtime_state: "stopped",
            managed: true, auto_start: false, watchdog_enabled: true, created_at: "now", updated_at: "now",
        };
        globalThis.fetch = async (_input, init) => {
            headers = new Headers(init?.headers);
            return Response.json(instance);
        };
        const response = await new ServerClient().updateServer(instance.id, { name: "Test" }, 7);
        expect(response.success).toBe(true);
        expect(headers.get("If-Match")).toBe('"7"');
    });
});
