import { z } from "zod";
import {
    CreateDiscordWebhook,
    DiscordWebhook,
    DiscordWebhookListSchema,
    DiscordWebhookSchema,
    UpdateDiscordWebhook,
} from "@/schemas/operations";
import { BaseClient, ClientResponse } from "./base.client";

export class WebhooksClient extends BaseClient {
    async list(): Promise<ClientResponse<DiscordWebhook[]>> {
        return this.request("/webhooks", DiscordWebhookListSchema);
    }

    async create(input: CreateDiscordWebhook): Promise<ClientResponse<DiscordWebhook>> {
        return this.request("/webhooks", DiscordWebhookSchema, {
            method: "POST",
            body: JSON.stringify(input),
        });
    }

    async update(id: string, input: UpdateDiscordWebhook, version: number): Promise<ClientResponse<DiscordWebhook>> {
        return this.request(`/webhooks/${encodeURIComponent(id)}`, DiscordWebhookSchema, {
            method: "PUT",
            headers: { "If-Match": `"${version}"` },
            body: JSON.stringify(input),
        });
    }

    async delete(id: string): Promise<ClientResponse<void>> {
        return this.request(`/webhooks/${encodeURIComponent(id)}`, z.undefined(), { method: "DELETE" });
    }
}
