import { z } from "zod";
import { GameProfile, GameProfileSchema } from "@/schemas/api";
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
