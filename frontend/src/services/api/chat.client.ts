import { z } from "zod";
import { ChatDraftSchema, ChatMessage, ChatMessageSchema, ChatPage, ChatPageSchema, OperationSuccessSchema } from "@/schemas/operations";
import { BaseClient, ClientResponse } from "./base.client";
import { queryString } from "./query";

export class ChatClient extends BaseClient {
    list(beforeId?: string, limit = 50): Promise<ClientResponse<ChatPage>> {
        return this.request(`/chat${queryString({ before_id: beforeId, limit })}`, ChatPageSchema);
    }

    create(body: string): Promise<ClientResponse<ChatMessage>> {
        const payload = ChatDraftSchema.parse({ body });
        return this.request("/chat", ChatMessageSchema, {
            method: "POST",
            body: JSON.stringify(payload),
        });
    }

    remove(id: string): Promise<ClientResponse<z.infer<typeof OperationSuccessSchema>>> {
        return this.request(`/chat/${encodeURIComponent(id)}`, OperationSuccessSchema, { method: "DELETE" });
    }
}
