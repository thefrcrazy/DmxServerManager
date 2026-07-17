import { PlayerSnapshot, PlayerSnapshotSchema } from "@/schemas/operations";
import { BaseClient, ClientResponse } from "./base.client";

export class PlayersClient extends BaseClient {
    snapshot(instanceId: string): Promise<ClientResponse<PlayerSnapshot>> {
        return this.request(
            `/servers/${encodeURIComponent(instanceId)}/players`,
            PlayerSnapshotSchema,
        );
    }
}
