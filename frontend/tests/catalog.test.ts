import { afterEach, describe, expect, test } from "bun:test";
import { sha256Package } from "../src/components/features/administration/CatalogManagement";
import { applyThemeTokens, DEFAULT_THEME_TOKENS } from "../src/constants/theme";
import {
    ActiveThemeSchema,
    CatalogPackageSchema,
    ThemeSelectionSchema,
    type ThemeTokens,
} from "../src/schemas/catalog";
import { setCsrfToken } from "../src/services/api/base.client";
import { CatalogClient } from "../src/services/api/catalog.client";

const originalFetch = globalThis.fetch;
const originalDocument = Object.getOwnPropertyDescriptor(globalThis, "document");

const activeTheme = {
    selection: { kind: "catalog", package_id: "theme-midnight", revision: 2 },
    tokens: DEFAULT_THEME_TOKENS,
    assets: {
        logo: {
            url: "/api/v1/catalog/theme/theme-midnight/revisions/2/assets/logo",
            sha256: "a".repeat(64),
            media_type: "image/png",
            size_bytes: 256,
        },
        preview: null,
    },
    version: 4,
    updated_at: "2026-07-13T12:00:00Z",
} as const;

afterEach(() => {
    globalThis.fetch = originalFetch;
    setCsrfToken(null);
    if (originalDocument) Object.defineProperty(globalThis, "document", originalDocument);
    else Reflect.deleteProperty(globalThis, "document");
});

describe("catalogue et thèmes fermés", () => {
    test("calcule le SHA-256 exact du .dmxpack avant l'upload", async () => {
        const file = new File(["abc"], "theme-midnight.dmxpack", { type: "application/vnd.dmxpack+zip" });
        expect(await sha256Package(file)).toBe("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    });

    test("accepte seulement les tokens connus et des assets PNG locaux authentifiés", () => {
        expect(ActiveThemeSchema.safeParse(activeTheme).success).toBe(true);
        expect(ActiveThemeSchema.safeParse({
            ...activeTheme,
            tokens: { ...activeTheme.tokens, css: "body{display:none}" },
        }).success).toBe(false);
        expect(ActiveThemeSchema.safeParse({
            ...activeTheme,
            assets: { ...activeTheme.assets, logo: { ...activeTheme.assets.logo!, url: "https://evil.example/logo.png" } },
        }).success).toBe(false);
        expect(ThemeSelectionSchema.safeParse({ kind: "default", package_id: "theme-evil" }).success).toBe(false);
    });

    test("applique uniquement le mapping CSS autorisé et refuse un token supplémentaire", () => {
        const values = new Map<string, string>();
        Object.defineProperty(globalThis, "document", {
            configurable: true,
            value: { documentElement: { style: { setProperty: (name: string, value: string) => values.set(name, value) } } },
        });

        expect(applyThemeTokens(DEFAULT_THEME_TOKENS)).toBe(true);
        expect(values.get("--color-bg-primary")).toBe("#000000");
        expect(values.get("--color-accent-rgb")).toBe("58, 130, 246");
        expect([...values.keys()].every((key) => /^--color-(?:accent|bg-(?:primary|secondary|tertiary|elevated)|border(?:-hover)?|text-(?:primary|secondary|muted|inverse)|success|warning|danger|info)(?:-rgb)?$/.test(key))).toBe(true);

        values.clear();
        const hostile = { ...DEFAULT_THEME_TOKENS, "background-image": "url(https://evil.example)" } as unknown as ThemeTokens;
        expect(applyThemeTokens(hostile)).toBe(false);
        expect(values.size).toBe(0);
    });

    test("envoie la sélection exacte avec If-Match, CSRF et sans Authorization", async () => {
        let captured: RequestInit | undefined;
        globalThis.fetch = async (_input, init) => {
            captured = init;
            return Response.json(activeTheme, { headers: { ETag: '"4"' } });
        };
        setCsrfToken("csrf-catalog");

        const response = await new CatalogClient().selectTheme(
            { kind: "catalog", package_id: "theme-midnight", revision: 2 },
            3,
        );

        expect(response.success).toBe(true);
        const headers = new Headers(captured?.headers);
        expect(headers.get("If-Match")).toBe('"3"');
        expect(headers.get("X-CSRF-Token")).toBe("csrf-catalog");
        expect(headers.has("Authorization")).toBe(false);
        expect(JSON.parse(String(captured?.body))).toEqual({
            kind: "catalog",
            package_id: "theme-midnight",
            revision: 2,
        });
    });

    test("refuse un DTO catalogue dont le type, l'identité ou le manifeste divergent", () => {
        const packageDto = {
            id: "theme-midnight",
            revision: 1,
            kind: "theme",
            schema_version: 1,
            name: "Midnight",
            description: "Thème sombre accessible.",
            archive_sha256: "b".repeat(64),
            archive_size_bytes: 1024,
            content_size_bytes: 128,
            manifest: {
                format: "dmxpack",
                schema_version: 1,
                id: "theme-midnight",
                revision: 1,
                name: "Midnight",
                description: "Thème sombre accessible.",
                content: { kind: "theme", tokens: "tokens.json", logo: null, preview: null },
                files: [{ path: "tokens.json", sha256: "c".repeat(64), size_bytes: 128, media_type: "application/json" }],
            },
            files: [{ role: "tokens", path: "tokens.json", media_type: "application/json", sha256: "c".repeat(64), size_bytes: 128 }],
            theme_tokens: DEFAULT_THEME_TOKENS,
            compatibility_status: "unverified",
            created_at: "2026-07-13T12:00:00Z",
        } as const;
        expect(CatalogPackageSchema.safeParse(packageDto).success).toBe(true);
        expect(CatalogPackageSchema.safeParse({ ...packageDto, id: "steam-midnight" }).success).toBe(false);
        expect(CatalogPackageSchema.safeParse({ ...packageDto, manifest: { ...packageDto.manifest, revision: 2 } }).success).toBe(false);
    });
});
