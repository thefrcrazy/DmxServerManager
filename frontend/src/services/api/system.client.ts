import { HealthResponse, HealthResponseSchema } from "@/schemas/api";
import { BaseClient, ClientResponse } from "./base.client";

export class SystemClient extends BaseClient {
    async health(): Promise<ClientResponse<HealthResponse>> {
        return this.request("/health", HealthResponseSchema, { skipAuth: true });
    }
}
