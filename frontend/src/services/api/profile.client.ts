import { z } from "zod";
import {
    GameProfile,
    GameProfileSchema,
    ProfileVersionCatalog,
    ProfileVersionCatalogSchema,
} from "@/schemas/api";
import {
    CreateSteamProfile,
    OperationSuccessSchema,
    SteamProfileDefinition,
    SteamProfileRevisionListSchema,
} from "@/schemas/operations";
import { BaseClient, ClientResponse } from "./base.client";

export class ProfileClient extends BaseClient {
    async getProfiles(): Promise<ClientResponse<GameProfile[]>> {
        return this.request("/game-profiles", z.array(GameProfileSchema));
    }

    async getRevisions(id: string): Promise<ClientResponse<GameProfile[]>> {
        return this.request(`/game-profiles/${encodeURIComponent(id)}/revisions`, SteamProfileRevisionListSchema);
    }

    async getVersionCatalog(id: string, gameVersion?: string, loader?: string): Promise<ClientResponse<ProfileVersionCatalog>> {
        const params = new URLSearchParams();
        if (gameVersion) params.set("game_version", gameVersion);
        if (loader) params.set("loader", loader);
        const query = params.size > 0 ? `?${params.toString()}` : "";
        return this.request(
            `/game-profiles/${encodeURIComponent(id)}/version-catalog${query}`,
            ProfileVersionCatalogSchema,
        );
    }

    async createSteam(input: CreateSteamProfile): Promise<ClientResponse<GameProfile>> {
        return this.request("/game-profiles/steam", GameProfileSchema, {
            method: "POST",
            body: JSON.stringify(input),
        });
    }

    async reviseSteam(id: string, definition: SteamProfileDefinition, revision: number): Promise<ClientResponse<GameProfile>> {
        return this.request(`/game-profiles/steam/${encodeURIComponent(id)}`, GameProfileSchema, {
            method: "PUT",
            headers: { "If-Match": `"${revision}"` },
            body: JSON.stringify(definition),
        });
    }

    async deleteSteam(id: string): Promise<ClientResponse<z.infer<typeof OperationSuccessSchema>>> {
        return this.request(`/game-profiles/steam/${encodeURIComponent(id)}`, OperationSuccessSchema, { method: "DELETE" });
    }
}
