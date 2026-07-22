import { expect, test } from "@playwright/test";
import { AuthResponseSchema, HealthResponseSchema, ProblemDetailsSchema } from "../src/schemas/api";
import { REAL_E2E_BASE_URL, REAL_E2E_SETUP_TOKEN } from "./runtime";

const OWNER_PASSWORD = "Owner-Real-E2E-2026!";
const VIEWER_PASSWORD = "Viewer-Real-E2E-2026!";
const VIEWER_PERMANENT_PASSWORD = "Viewer-Permanent-E2E-2026!";
const apiUrl = (path: string): string => new URL(path, `${REAL_E2E_BASE_URL}/`).toString();

test("le vrai Axum, SQLite et la SPA appliquent setup, session, CSRF et RBAC", async ({ page, playwright }) => {
    const health = await page.request.get("/api/v1/health");
    expect(health.status()).toBe(200);
    const healthBody = HealthResponseSchema.parse(await health.json());
    expect(healthBody).toMatchObject({
        status: "ok",
        service: "dmx-server-manager",
    });
    expect(healthBody.version).toBe("1.1.6");

    const anonymousPrivate = await page.request.get("/api/v1/users");
    expect(anonymousPrivate.status()).toBe(401);
    const anonymousProblem = ProblemDetailsSchema.parse(await anonymousPrivate.json());
    expect(anonymousProblem.trace_id).toBeTruthy();

    await page.goto("/");
    await expect(page).toHaveURL(/\/setup$/);
    await expect(page.getByRole("heading", { name: "DmxServerManager" })).toBeVisible();
    await page.getByLabel("Nom d'utilisateur").fill("real-owner");
    await page.getByLabel("Mot de passe", { exact: true }).fill(OWNER_PASSWORD);
    await page.getByLabel("Confirmer le mot de passe").fill(OWNER_PASSWORD);
    await page.getByLabel("Jeton d’installation distant (optionnel)").fill(REAL_E2E_SETUP_TOKEN);

    const setupResponsePromise = page.waitForResponse((response) => (
        response.request().method() === "POST"
        && new URL(response.url()).pathname === "/api/v1/auth/setup"
    ));
    await page.getByRole("button", { name: "Terminer l'installation" }).click();
    const setupResponse = await setupResponsePromise;
    expect(setupResponse.status()).toBe(201);
    const setup = AuthResponseSchema.parse(await setupResponse.json());
    expect(setup.user.role).toBe("owner");
    expect(setup.user.permissions).toContain("*");
    expect(setup.csrf_token).toMatch(/^[A-Za-z0-9_-]{40,}$/);
    expect(setupResponse.request().headers().authorization).toBeUndefined();
    const setCookie = await setupResponse.headerValue("set-cookie");
    expect(setCookie).toContain("dmx_session=");
    expect(setCookie).toContain("HttpOnly");
    expect(setCookie).toContain("SameSite=Strict");
    expect(setCookie).not.toContain("; Secure");

    await expect(page).toHaveURL(/\/dashboard$/);
    await expect(page.getByRole("region", { name: "Vue d’ensemble opérationnelle" })).toBeVisible();
    await expect(page.getByRole("heading", { name: "Santé des serveurs" })).toBeVisible();
    const sessionCookie = (await page.context().cookies())
        .find((cookie) => cookie.name === "dmx_session");
    expect(sessionCookie).toBeDefined();
    expect(sessionCookie).toEqual(expect.objectContaining({
        httpOnly: true,
        sameSite: "Strict",
        secure: false,
        path: "/api/v1",
    }));
    expect(sessionCookie?.value).toMatch(/^[A-Za-z0-9_-]{40,}$/);
    expect(sessionCookie?.value).not.toContain(".");
    const localStorageEntries = await page.evaluate(() => Object.entries(localStorage));
    expect(localStorageEntries.some(([key]) => /(?:csrf|jwt|session|token)/i.test(key))).toBe(false);
    expect(JSON.stringify(localStorageEntries)).not.toContain(sessionCookie?.value ?? "missing-session");

    const meResponse = await page.request.get("/api/v1/auth/me");
    expect(meResponse.status()).toBe(200);
    const currentSession = AuthResponseSchema.parse(await meResponse.json());
    const ownerUsers = await page.request.get("/api/v1/users");
    expect(ownerUsers.status()).toBe(200);
    expect((await ownerUsers.json()) as Array<{ username: string }>).toContainEqual(
        expect.objectContaining({ username: "real-owner" }),
    );

    const viewerPayload = {
        username: "real-viewer",
        password: VIEWER_PASSWORD,
        role_id: "viewer",
        language: "fr",
    };
    const missingCsrf = await page.request.post("/api/v1/users", { data: viewerPayload });
    expect(missingCsrf.status()).toBe(403);
    expect(ProblemDetailsSchema.parse(await missingCsrf.json()).trace_id).toBeTruthy();

    const createViewer = await page.request.post("/api/v1/users", {
        data: viewerPayload,
        headers: { "X-CSRF-Token": currentSession.csrf_token },
    });
    expect(createViewer.status()).toBe(201);
    const createdViewer = await createViewer.json() as Record<string, unknown>;
    expect(createdViewer).toEqual(expect.objectContaining({
        username: "real-viewer",
        role_id: "viewer",
        must_change_password: true,
    }));
    expect(createdViewer).not.toHaveProperty("password");
    expect(createdViewer).not.toHaveProperty("password_hash");

    const viewerApi = await playwright.request.newContext();
    try {
        const viewerLogin = await viewerApi.post(apiUrl("/api/v1/auth/login"), {
            data: { username: "real-viewer", password: VIEWER_PASSWORD },
        });
        expect(viewerLogin.status()).toBe(200);
        const viewerSession = AuthResponseSchema.parse(await viewerLogin.json());
        expect(viewerSession.user.role).toBe("viewer");
        expect(viewerSession.user.permissions).not.toContain("user.read");
        expect(viewerSession.user.must_change_password).toBe(true);

        const blockedProfiles = await viewerApi.get(apiUrl("/api/v1/game-profiles"));
        expect(blockedProfiles.status()).toBe(403);
        expect(ProblemDetailsSchema.parse(await blockedProfiles.json()).code).toBe("AUTH_009");

        const viewerMe = await viewerApi.get(apiUrl("/api/v1/auth/me"));
        expect(viewerMe.status()).toBe(200);
        const viewerCurrentSession = AuthResponseSchema.parse(await viewerMe.json());
        const passwordChange = await viewerApi.put(apiUrl("/api/v1/auth/password"), {
            data: {
                current_password: VIEWER_PASSWORD,
                new_password: VIEWER_PERMANENT_PASSWORD,
            },
            headers: { "X-CSRF-Token": viewerCurrentSession.csrf_token },
        });
        expect(passwordChange.status()).toBe(200);
        expect((await viewerApi.get(apiUrl("/api/v1/game-profiles"))).status()).toBe(401);

        const viewerRelogin = await viewerApi.post(apiUrl("/api/v1/auth/login"), {
            data: { username: "real-viewer", password: VIEWER_PERMANENT_PASSWORD },
        });
        expect(viewerRelogin.status()).toBe(200);
        expect(AuthResponseSchema.parse(await viewerRelogin.json()).user.must_change_password).toBe(false);

        const allowedProfiles = await viewerApi.get(apiUrl("/api/v1/game-profiles"));
        expect(allowedProfiles.status()).toBe(200);
        expect(Array.isArray(await allowedProfiles.json())).toBe(true);

        const forbiddenUsers = await viewerApi.get(apiUrl("/api/v1/users"));
        expect(forbiddenUsers.status()).toBe(403);
        const forbiddenProblem = ProblemDetailsSchema.parse(await forbiddenUsers.json());
        expect(forbiddenProblem.trace_id).toBeTruthy();
    } finally {
        await viewerApi.dispose();
    }
});
