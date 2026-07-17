import {
    ConfigFileDocument,
    ConfigFileDocumentSchema,
    ConfigFileListSchema,
    ConfigFileSummary,
    OperationSuccessSchema,
} from "@/schemas/operations";
import { BaseClient, ClientResponse } from "./base.client";
import { queryString } from "./query";

function endpoint(instanceId: string, path?: string): string {
    const base = `/servers/${encodeURIComponent(instanceId)}/config-files`;
    return path === undefined ? base : `${base}/text${queryString({ path })}`;
}

export class ConfigClient extends BaseClient {
    async list(instanceId: string): Promise<ClientResponse<{ items: ConfigFileSummary[]; pending_count: number }>> {
        return this.request(endpoint(instanceId), ConfigFileListSchema);
    }

    async read(instanceId: string, path: string): Promise<ClientResponse<ConfigFileDocument>> {
        return this.request(endpoint(instanceId, path), ConfigFileDocumentSchema);
    }

    async queue(
        instanceId: string,
        path: string,
        content: string,
        expectedSha256: string | null,
    ): Promise<ClientResponse<ConfigFileDocument>> {
        return this.request(endpoint(instanceId, path), ConfigFileDocumentSchema, {
            method: "PUT",
            body: JSON.stringify({ content, expected_sha256: expectedSha256 }),
        });
    }

    async cancel(instanceId: string, path: string) {
        return this.request(endpoint(instanceId, path), OperationSuccessSchema, { method: "DELETE" });
    }
}
