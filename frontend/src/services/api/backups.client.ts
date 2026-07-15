import { z } from "zod";
import { Job } from "@/schemas/api";
import { AcceptedJobSchema, Backup, BackupSchema, OperationSuccessSchema } from "@/schemas/operations";
import { API_BASE_URL, BaseClient, ClientResponse } from "./base.client";
import { queryString } from "./query";

export class BackupsClient extends BaseClient {
    list(instanceId: string): Promise<ClientResponse<Backup[]>> {
        return this.request(`/backups${queryString({ instance_id: instanceId })}`, z.array(BackupSchema));
    }

    get(id: string): Promise<ClientResponse<Backup>> {
        return this.request(`/backups/${encodeURIComponent(id)}`, BackupSchema);
    }

    create(instanceId: string, idempotencyKey: string): Promise<ClientResponse<Job>> {
        return this.request("/backups", AcceptedJobSchema, {
            method: "POST",
            headers: { "Idempotency-Key": idempotencyKey },
            body: JSON.stringify({ instance_id: instanceId }),
        });
    }

    restore(id: string, idempotencyKey: string): Promise<ClientResponse<Job>> {
        return this.request(`/backups/${encodeURIComponent(id)}/restore`, AcceptedJobSchema, {
            method: "POST",
            headers: { "Idempotency-Key": idempotencyKey },
        });
    }

    remove(id: string): Promise<ClientResponse<z.infer<typeof OperationSuccessSchema>>> {
        return this.request(`/backups/${encodeURIComponent(id)}`, OperationSuccessSchema, { method: "DELETE" });
    }

    downloadUrl(id: string): string {
        return `${API_BASE_URL}/backups/${encodeURIComponent(id)}/download`;
    }
}
