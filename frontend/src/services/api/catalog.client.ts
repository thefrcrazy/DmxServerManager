import { z } from "zod";
import { JobSchema, SuccessResponseSchema } from "@/schemas/api";
import {
    ActiveThemeSchema,
    CatalogKind,
    CatalogPackageSchema,
    ThemeSelection,
} from "@/schemas/catalog";
import { BaseClient } from "./base.client";

export class CatalogClient extends BaseClient {
    list(kind?: CatalogKind) {
        const query = kind ? `?kind=${encodeURIComponent(kind)}` : "";
        return this.request(`/catalog${query}`, z.array(CatalogPackageSchema));
    }

    revisions(kind: CatalogKind, id: string) {
        return this.request(
            `/catalog/${encodeURIComponent(kind)}/${encodeURIComponent(id)}/revisions`,
            z.array(CatalogPackageSchema),
        );
    }

    importPackage(file: File, sha256: string, idempotencyKey: string) {
        return this.request("/catalog/import", JobSchema, {
            method: "POST",
            headers: {
                "Content-Type": "application/vnd.dmxpack+zip",
                "X-Dmx-Package-Sha256": sha256,
                "Idempotency-Key": idempotencyKey,
            },
            body: file,
        });
    }

    deleteRevision(kind: CatalogKind, id: string, revision: number) {
        return this.request(
            `/catalog/${encodeURIComponent(kind)}/${encodeURIComponent(id)}/revisions/${revision}`,
            SuccessResponseSchema,
            { method: "DELETE" },
        );
    }

    activeTheme() {
        return this.request("/catalog/theme", ActiveThemeSchema);
    }

    selectTheme(selection: ThemeSelection, version: number) {
        return this.request("/catalog/theme", ActiveThemeSchema, {
            method: "PUT",
            headers: { "If-Match": `"${version}"` },
            body: JSON.stringify(selection),
        });
    }
}
