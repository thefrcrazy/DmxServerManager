import { afterEach, describe, expect, test } from "bun:test";
import { PERMISSION_CATALOG } from "../src/constants/permissions";
import { AdminClient } from "../src/services/api/admin.client";
import { setCsrfToken } from "../src/services/api/base.client";

const originalFetch = globalThis.fetch;
const NOW = "2026-07-13T12:00:00.000Z";
const USER = {
    id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    username: "operator-one",
    role_id: "operator",
    role_name: "Operator",
    is_active: true,
    language: "fr",
    accent_color: "#3a82f6",
    must_change_password: true,
    last_login_at: null,
    created_at: NOW,
    updated_at: NOW,
};

afterEach(() => {
    globalThis.fetch = originalFetch;
    setCsrfToken(null);
});

describe("client d’administration", () => {
    test("rejette tout champ secret inattendu dans un compte", async () => {
        globalThis.fetch = async () =>
            Response.json([{ ...USER, password_hash: "argon2id-secret" }]); // gitleaks:allow

        const response = await new AdminClient().listUsers();

        expect(response.success).toBe(false);
        if (!response.success) expect(response.error.message).toContain("Réponse API invalide");
    });

    test("crée un compte via cookie et CSRF sans en-tête bearer", async () => {
        let input = "";
        let captured: RequestInit | undefined;
        globalThis.fetch = async (requestInput, init) => {
            input = String(requestInput);
            captured = init;
            return Response.json(USER, { status: 201 });
        };
        setCsrfToken("csrf-admin-test");

        const response = await new AdminClient().createUser({
            username: "operator-one",
            password: "Secure-Operator-2026!",
            role_id: "operator",
            language: "fr",
        });

        expect(response.success).toBe(true);
        expect(input).toBe("/api/v1/users");
        expect(captured?.credentials).toBe("same-origin");
        const headers = new Headers(captured?.headers);
        expect(headers.get("X-CSRF-Token")).toBe("csrf-admin-test");
        expect(headers.has("Authorization")).toBe(false);
        expect(JSON.parse(String(captured?.body))).toEqual({
            username: "operator-one",
            password: "Secure-Operator-2026!",
            role_id: "operator",
            language: "fr",
        });
    });

    test("encode les identifiants et envoie une liste fermée pour une affectation", async () => {
        let input = "";
        let captured: RequestInit | undefined;
        globalThis.fetch = async (requestInput, init) => {
            input = String(requestInput);
            captured = init;
            return Response.json({
                instance_id: "11111111-1111-4111-8111-111111111111",
                instance_name: "Survie",
                permissions: ["server.read"],
                created_at: NOW,
            });
        };
        setCsrfToken("csrf-admin-test");

        const response = await new AdminClient().setGrant(
            "user/id",
            "11111111-1111-4111-8111-111111111111",
            ["server.read"],
        );

        expect(response.success).toBe(true);
        expect(input).toBe("/api/v1/users/user%2Fid/instances/11111111-1111-4111-8111-111111111111");
        expect(captured?.method).toBe("PUT");
        expect(JSON.parse(String(captured?.body))).toEqual({ permissions: ["server.read"] });
        expect(PERMISSION_CATALOG.find((permission) => permission.id === "server.console.write")?.high_risk).toBe(true);
        expect(PERMISSION_CATALOG.find((permission) => permission.id === "user.update")?.instance_scoped).toBe(false);
        expect(PERMISSION_CATALOG.find((permission) => permission.id === "server.backup.read")?.instance_scoped).toBe(true);
    });
});
