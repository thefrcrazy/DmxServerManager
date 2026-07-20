import {
    NetworkSettingsSchema,
    type NetworkSettings,
} from "@/schemas/operations";
import { BaseClient, type ClientResponse } from "./base.client";

export class PanelClient extends BaseClient {
    network(): Promise<ClientResponse<NetworkSettings>> {
        return this.request("/panel/network", NetworkSettingsSchema);
    }

    updateNetwork(advertisedGameHost: string | null, expectedVersion: number): Promise<ClientResponse<NetworkSettings>> {
        return this.request("/panel/network", NetworkSettingsSchema, {
            method: "PUT",
            body: JSON.stringify({
                advertised_game_host: advertisedGameHost,
                expected_version: expectedVersion,
            }),
        });
    }
}
