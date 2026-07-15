import { afterEach, describe, expect, test } from "bun:test";
import { z } from "zod";
import { BaseClient, apiFetch, setCsrfToken } from "../src/services/api/base.client";

const originalFetch = globalThis.fetch;
const originalWindowDescriptor = Object.getOwnPropertyDescriptor(globalThis, "window");

class TestClient extends BaseClient {
    requestProtected() {
        return this.request("/game-profiles", z.unknown());
    }
}

afterEach(() => {
    globalThis.fetch = originalFetch;
    setCsrfToken(null);
    if (originalWindowDescriptor) {
        Object.defineProperty(globalThis, "window", originalWindowDescriptor);
    } else {
        Reflect.deleteProperty(globalThis, "window");
    }
});

describe("transport API", () => {
    test("utilise le cookie same-origin et retire tout bearer legacy", async () => {
        let captured: RequestInit | undefined;
        globalThis.fetch = async (_input, init) => {
            captured = init;
            return new Response(null, { status: 204 });
        };

        await apiFetch("/api/v1/servers", { headers: { Authorization: "Bearer legacy" } });

        expect(captured?.credentials).toBe("same-origin");
        expect(new Headers(captured?.headers).has("Authorization")).toBe(false);
    });

    test("ajoute le CSRF uniquement aux mutations", async () => {
        const requests: RequestInit[] = [];
        globalThis.fetch = async (_input, init) => {
            requests.push(init ?? {});
            return new Response(null, { status: 204 });
        };
        setCsrfToken("csrf-test");

        await apiFetch("/api/v1/servers");
        await apiFetch("/api/v1/servers", { method: "POST", body: "{}" });

        expect(new Headers(requests[0]?.headers).has("X-CSRF-Token")).toBe(false);
        expect(new Headers(requests[1]?.headers).get("X-CSRF-Token")).toBe("csrf-test");
        expect(new Headers(requests[1]?.headers).get("Content-Type")).toBe("application/json");
    });

    test("refuse toute destination hors de l’API same-origin", async () => {
        setCsrfToken("must-not-leak");
        let called = false;
        globalThis.fetch = async () => {
            called = true;
            return new Response(null, { status: 204 });
        };

        await expect(apiFetch("https://evil.example/api/v1/servers", { method: "POST", body: "{}" }))
            .rejects.toThrow("URL API hors périmètre");
        await expect(apiFetch("/api/v1/../outside", { method: "POST", body: "{}" }))
            .rejects.toThrow("URL API hors périmètre");
        expect(called).toBe(false);
    });

    test("ne marque pas les uploads binaires comme JSON", async () => {
        let headers = new Headers();
        globalThis.fetch = async (_input, init) => {
            headers = new Headers(init?.headers);
            return new Response(null, { status: 204 });
        };

        await apiFetch("/api/v1/files/content?instance_id=id&path=file.bin", {
            method: "PUT",
            body: new Blob(["binary"]),
        });

        expect(headers.has("Content-Type")).toBe(false);
    });

    test("signale au contexte une session désormais limitée au changement de mot de passe", async () => {
        const eventTypes: string[] = [];
        Object.defineProperty(globalThis, "window", {
            configurable: true,
            value: {
                dispatchEvent: (event: Event) => {
                    eventTypes.push(event.type);
                    return true;
                },
            },
        });
        globalThis.fetch = async () => Response.json({
            type: "about:blank",
            title: "auth.password_change_required",
            status: 403,
            code: "AUTH_009",
            trace_id: "trace-password-change",
        }, {
            status: 403,
            headers: { "Content-Type": "application/problem+json" },
        });

        const response = await new TestClient().requestProtected();

        expect(response.success).toBe(false);
        expect(eventTypes).toEqual(["dmx-password-change-required"]);
    });
});
