import { z } from "zod";
import {
    FileWriteResultSchema,
    ManagedFileEntry,
    ManagedFileListSchema,
    TextFileSchema,
    OperationSuccessSchema,
} from "@/schemas/operations";
import { API_BASE_URL, BaseClient, ClientResponse } from "./base.client";
import { queryString } from "./query";

function fileQuery(instanceId: string, path: string): string {
    return queryString({ instance_id: instanceId, path: path || undefined });
}

export class FilesClient extends BaseClient {
    async list(instanceId: string, path = ""): Promise<ClientResponse<ManagedFileEntry[]>> {
        const response = await this.request(`/files${fileQuery(instanceId, path)}`, ManagedFileListSchema);
        if (!response.success) return response;
        return { ...response, data: response.data.items };
    }

    readText(instanceId: string, path: string): Promise<ClientResponse<{ content: string }>> {
        return this.request(`/files/text${fileQuery(instanceId, path)}`, TextFileSchema);
    }

    writeText(instanceId: string, path: string, content: string): Promise<ClientResponse<{ bytes_written: number }>> {
        return this.request(`/files/text${fileQuery(instanceId, path)}`, FileWriteResultSchema, {
            method: "PUT",
            body: JSON.stringify({ content }),
        });
    }

    upload(instanceId: string, path: string, body: Blob): Promise<ClientResponse<{ bytes_written: number }>> {
        return this.request(`/files/content${fileQuery(instanceId, path)}`, FileWriteResultSchema, {
            method: "PUT",
            headers: { "Content-Type": "application/octet-stream" },
            body,
        });
    }

    createDirectory(instanceId: string, path: string): Promise<ClientResponse<z.infer<typeof OperationSuccessSchema>>> {
        return this.request(`/files/directories${fileQuery(instanceId, path)}`, OperationSuccessSchema, { method: "POST" });
    }

    remove(instanceId: string, path: string): Promise<ClientResponse<z.infer<typeof OperationSuccessSchema>>> {
        return this.request(`/files${fileQuery(instanceId, path)}`, OperationSuccessSchema, { method: "DELETE" });
    }

    downloadUrl(instanceId: string, path: string): string {
        return `${API_BASE_URL}/files/content${fileQuery(instanceId, path)}`;
    }
}
