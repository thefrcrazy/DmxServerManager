import { PanelReleaseStatus, PanelReleaseStatusSchema } from "@/schemas/releases";
import { BaseClient, ClientResponse } from "./base.client";

export class ReleasesClient extends BaseClient {
    async status(): Promise<ClientResponse<PanelReleaseStatus>> {
        return this.request("/releases/panel", PanelReleaseStatusSchema);
    }

    async check(): Promise<ClientResponse<PanelReleaseStatus>> {
        return this.request("/releases/panel/check", PanelReleaseStatusSchema, { method: "POST" });
    }
}
