import { z } from "zod";
import {
    Instance,
    InstanceSchema,
    ConnectionInfo,
    ConnectionInfoSchema,
    Job,
    JobSchema,
    SecretStatusListSchema,
    SecretStatusSchema,
    SuccessResponseSchema,
} from "@/schemas/api";
import { BaseClient, ClientResponse } from "./base.client";

export interface CreateServerInput {
    name: string;
    profile_id: string;
    settings: Record<string, unknown>;
    secrets?: Record<string, string>;
    auto_start?: boolean;
    watchdog_enabled?: boolean;
}

export interface UpdateServerInput {
    name?: string;
    settings?: Record<string, unknown>;
    auto_start?: boolean;
    watchdog_enabled?: boolean;
}

export type ServerAction = "install" | "start" | "stop" | "restart" | "kill";

const ActionResponseSchema = z.union([
    JobSchema,
    z.object({ job: JobSchema }).transform(({ job }) => job),
]);

export type ServerLogSource = "install" | "console";

const LogHistoryResponseSchema = z.object({
    source: z.enum(["install", "console"]),
    items: z.array(z.object({
        stream: z.enum(["install", "install_error", "console", "console_error"]),
        message: z.string(),
    })),
});

export class ServerClient extends BaseClient {
    async getServers(): Promise<ClientResponse<Instance[]>> {
        return this.request("/servers", z.array(InstanceSchema));
    }

    async getServer(id: string): Promise<ClientResponse<Instance>> {
        return this.request(`/servers/${encodeURIComponent(id)}`, InstanceSchema);
    }

    async getConnection(id: string): Promise<ClientResponse<ConnectionInfo>> {
        return this.request(`/servers/${encodeURIComponent(id)}/connection`, ConnectionInfoSchema);
    }

    async createServer(data: CreateServerInput): Promise<ClientResponse<Instance>> {
        return this.request("/servers", InstanceSchema, {
            method: "POST",
            body: JSON.stringify(data),
        });
    }

    async updateServer(id: string, data: UpdateServerInput, configVersion: number): Promise<ClientResponse<Instance>> {
        return this.request(`/servers/${encodeURIComponent(id)}`, InstanceSchema, {
            method: "PATCH",
            headers: { "If-Match": `"${configVersion}"` },
            body: JSON.stringify(data),
        });
    }

    async deleteServer(id: string): Promise<ClientResponse<z.infer<typeof SuccessResponseSchema>>> {
        return this.request(`/servers/${encodeURIComponent(id)}`, SuccessResponseSchema, { method: "DELETE" });
    }

    async runAction(id: string, action: ServerAction): Promise<ClientResponse<Job>> {
        return this.request(`/servers/${encodeURIComponent(id)}/actions/${action}`, ActionResponseSchema, { method: "POST" });
    }

    startServer(id: string) { return this.runAction(id, "start"); }
    stopServer(id: string) { return this.runAction(id, "stop"); }
    restartServer(id: string) { return this.runAction(id, "restart"); }
    killServer(id: string) { return this.runAction(id, "kill"); }
    reinstallServer(id: string) { return this.runAction(id, "install"); }

    async sendCommand(id: string, command: string): Promise<ClientResponse<{ accepted: boolean }>> {
        return this.request(
            `/servers/${encodeURIComponent(id)}/console`,
            z.object({ accepted: z.boolean() }),
            { method: "POST", body: JSON.stringify({ command }) },
        );
    }

    async getLogHistory(id: string, source: ServerLogSource): Promise<ClientResponse<z.infer<typeof LogHistoryResponseSchema>>> {
        const query = new URLSearchParams({ source, limit: source === "install" ? "10000" : "1000" });
        return this.request(
            `/servers/${encodeURIComponent(id)}/logs?${query.toString()}`,
            LogHistoryResponseSchema,
        );
    }

    async getSecrets(id: string) {
        return this.request(`/servers/${encodeURIComponent(id)}/secrets`, SecretStatusListSchema);
    }

    async setSecret(id: string, name: string, value: string) {
        return this.request(
            `/servers/${encodeURIComponent(id)}/secrets/${encodeURIComponent(name)}`,
            SecretStatusSchema,
            { method: "PUT", body: JSON.stringify({ value }) },
        );
    }

    async deleteSecret(id: string, name: string) {
        return this.request(
            `/servers/${encodeURIComponent(id)}/secrets/${encodeURIComponent(name)}`,
            SecretStatusSchema,
            { method: "DELETE" },
        );
    }
}
