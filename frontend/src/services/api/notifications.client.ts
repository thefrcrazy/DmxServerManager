import { z } from "zod";
import { NotificationPage, NotificationPageSchema, OperationSuccessSchema } from "@/schemas/operations";
import { BaseClient, ClientResponse } from "./base.client";
import { queryString } from "./query";

export class NotificationsClient extends BaseClient {
    list(options: { beforeId?: string; limit?: number; unreadOnly?: boolean } = {}): Promise<ClientResponse<NotificationPage>> {
        return this.request(`/notifications${queryString({
            before_id: options.beforeId,
            limit: options.limit ?? 50,
            unread_only: options.unreadOnly || undefined,
        })}`, NotificationPageSchema);
    }

    markRead(id: string): Promise<ClientResponse<z.infer<typeof OperationSuccessSchema>>> {
        return this.request(`/notifications/${encodeURIComponent(id)}/read`, OperationSuccessSchema, { method: "PUT" });
    }

    markAllRead(): Promise<ClientResponse<z.infer<typeof OperationSuccessSchema>>> {
        return this.request("/notifications/read-all", OperationSuccessSchema, { method: "POST" });
    }
}
