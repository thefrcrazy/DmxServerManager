import { z } from "zod";

const SafeReleaseUrlSchema = z.url().max(4_096).refine((value) => {
    try {
        const url = new URL(value);
        return url.protocol === "https:"
            && url.port === ""
            && url.username === ""
            && url.password === ""
            && url.search === ""
            && url.hash === ""
            && [
                "github.com",
                "raw.githubusercontent.com",
                "objects.githubusercontent.com",
                "release-assets.githubusercontent.com",
                "thefrcrazy.github.io",
            ].includes(url.hostname);
    } catch {
        return false;
    }
});

const Sha256Schema = z.string().regex(/^[a-f0-9]{64}$/);
const DateTimeSchema = z.iso.datetime({ offset: true });

export const NativeReleaseTargetSchema = z.object({
    kind: z.literal("native"),
    platform: z.enum(["linux-amd64", "windows-amd64"]),
    archive_url: SafeReleaseUrlSchema,
    archive_sha256: Sha256Schema,
    installer_url: SafeReleaseUrlSchema,
    installer_sha256: Sha256Schema,
    upgrade_command: z.string().min(1).max(8_192),
}).strict();

export const DockerReleaseTargetSchema = z.object({
    kind: z.literal("docker"),
    image: z.literal("ghcr.io/thefrcrazy/dmx-server-manager"),
    digest: z.string().regex(/^sha256:[a-f0-9]{64}$/),
    pull_command: z.string().min(1).max(8_192),
    apply_command: z.string().min(1).max(8_192),
}).strict();

export const PanelReleaseStatusSchema = z.object({
    configured: z.boolean(),
    current_version: z.string().min(1).max(64),
    deployment_mode: z.enum(["native", "docker"]),
    state: z.enum(["disabled", "never_checked", "checking", "up_to_date", "update_available", "check_failed"]),
    checked_at: DateTimeSchema.nullable(),
    latest: z.object({
        version: z.string().min(1).max(64),
        published_at: DateTimeSchema,
        notes_url: SafeReleaseUrlSchema,
        target: z.discriminatedUnion("kind", [NativeReleaseTargetSchema, DockerReleaseTargetSchema]),
    }).strict().nullable(),
    error_code: z.enum(["network", "response_too_large", "envelope_invalid", "signature_invalid", "manifest_invalid"]).nullable(),
}).strict();

export type PanelReleaseStatus = z.infer<typeof PanelReleaseStatusSchema>;
export type PanelReleaseTarget = NonNullable<PanelReleaseStatus["latest"]>["target"];
