import { z } from "zod";

export const CatalogKindSchema = z.enum(["steam_profile", "theme"]);
export type CatalogKind = z.infer<typeof CatalogKindSchema>;

const CatalogIdSchema = z.string()
    .min(7)
    .max(64)
    .regex(/^(?:steam|theme)-[a-z0-9]+(?:-[a-z0-9]+)*$/)
    .refine((value) => value !== "steam-custom");

const CatalogPathSchema = z.string()
    .min(1)
    .max(256)
    .regex(/^[a-z0-9._/-]+$/)
    .refine((value) => !value.startsWith("/") && !value.endsWith("/") && !value.includes("//"))
    .refine((value) => value.split("/").every((part) => part !== "." && part !== ".."));

export const ThemeColorSchema = z.string().regex(/^#[0-9a-f]{6}$/i);

export const ThemeTokensSchema = z.object({
    accent: ThemeColorSchema,
    bg_primary: ThemeColorSchema,
    bg_secondary: ThemeColorSchema,
    bg_tertiary: ThemeColorSchema,
    bg_elevated: ThemeColorSchema,
    border: ThemeColorSchema,
    border_hover: ThemeColorSchema,
    text_primary: ThemeColorSchema,
    text_secondary: ThemeColorSchema,
    text_muted: ThemeColorSchema,
    success: ThemeColorSchema,
    warning: ThemeColorSchema,
    danger: ThemeColorSchema,
    info: ThemeColorSchema,
}).strict();
export type ThemeTokens = z.infer<typeof ThemeTokensSchema>;

export const ThemeSelectionSchema = z.discriminatedUnion("kind", [
    z.object({ kind: z.literal("default") }).strict(),
    z.object({
        kind: z.literal("catalog"),
        package_id: CatalogIdSchema.refine((value) => value.startsWith("theme-")),
        revision: z.number().int().positive(),
    }).strict(),
]);
export type ThemeSelection = z.infer<typeof ThemeSelectionSchema>;

const ThemeAssetSchema = z.object({
    url: z.string().regex(/^\/api\/v1\/catalog\/theme\/theme-[a-z0-9]+(?:-[a-z0-9]+)*\/revisions\/[1-9][0-9]*\/assets\/(?:logo|preview)$/),
    sha256: z.string().regex(/^[0-9a-f]{64}$/),
    media_type: z.literal("image/png"),
    size_bytes: z.number().int().positive().max(2 * 1024 * 1024),
}).strict();

export const ActiveThemeSchema = z.object({
    selection: ThemeSelectionSchema,
    tokens: ThemeTokensSchema,
    assets: z.object({
        logo: ThemeAssetSchema.nullable(),
        preview: ThemeAssetSchema.nullable(),
    }).strict(),
    version: z.number().int().positive(),
    updated_at: z.string(),
}).strict();
export type ActiveTheme = z.infer<typeof ActiveThemeSchema>;

const CatalogFileDeclarationSchema = z.object({
    path: CatalogPathSchema,
    sha256: z.string().regex(/^[0-9a-f]{64}$/),
    size_bytes: z.number().int().positive(),
    media_type: z.enum(["application/json", "image/png"]),
}).strict();

const CatalogContentSchema = z.discriminatedUnion("kind", [
    z.object({
        kind: z.literal("steam_profile"),
        definition: CatalogPathSchema,
        settings_schema: CatalogPathSchema,
        ui_schema: CatalogPathSchema,
        icon: CatalogPathSchema.nullable(),
    }).strict(),
    z.object({
        kind: z.literal("theme"),
        tokens: CatalogPathSchema,
        logo: CatalogPathSchema.nullable(),
        preview: CatalogPathSchema.nullable(),
    }).strict(),
]);

const CatalogManifestSchema = z.object({
    format: z.literal("dmxpack"),
    schema_version: z.literal(1),
    id: CatalogIdSchema,
    revision: z.number().int().positive(),
    name: z.string().min(1).max(80),
    description: z.string().min(1).max(500),
    content: CatalogContentSchema,
    files: z.array(CatalogFileDeclarationSchema).min(1).max(63),
}).strict();

const CatalogFileSchema = z.object({
    role: z.enum(["definition", "settings_schema", "ui_schema", "tokens", "icon", "logo", "preview"]),
    path: CatalogPathSchema,
    media_type: z.enum(["application/json", "image/png"]),
    sha256: z.string().regex(/^[0-9a-f]{64}$/),
    size_bytes: z.number().int().positive(),
}).strict();

export const CatalogPackageSchema = z.object({
    id: CatalogIdSchema,
    revision: z.number().int().positive(),
    kind: CatalogKindSchema,
    schema_version: z.literal(1),
    name: z.string().min(1).max(80),
    description: z.string().min(1).max(500),
    archive_sha256: z.string().regex(/^[0-9a-f]{64}$/),
    archive_size_bytes: z.number().int().positive().max(16 * 1024 * 1024),
    content_size_bytes: z.number().int().positive().max(32 * 1024 * 1024),
    manifest: CatalogManifestSchema,
    files: z.array(CatalogFileSchema).min(1).max(63),
    theme_tokens: ThemeTokensSchema.nullable(),
    compatibility_status: z.literal("unverified"),
    created_at: z.string(),
}).strict().superRefine((value, context) => {
    const expectedPrefix = value.kind === "theme" ? "theme-" : "steam-";
    if (!value.id.startsWith(expectedPrefix)
        || value.manifest.id !== value.id
        || value.manifest.revision !== value.revision
        || value.manifest.content.kind !== value.kind
        || (value.kind === "theme") !== (value.theme_tokens !== null)) {
        context.addIssue({ code: "custom", message: "Inconsistent catalog package identity" });
    }
});
export type CatalogPackage = z.infer<typeof CatalogPackageSchema>;
