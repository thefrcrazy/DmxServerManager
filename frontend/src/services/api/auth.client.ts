import { z } from "zod";
import {
    AuthResponse,
    AuthResponseSchema,
    SetupStatus,
    SetupStatusSchema,
    SessionInfoSchema,
    SuccessResponseSchema,
    UserInfoSchema,
    type SessionInfo,
    type UserInfo,
} from "@/schemas/api";
import { BaseClient, ClientResponse, setCsrfToken } from "./base.client";

export class AuthClient extends BaseClient {
    async login(username: string, password: string): Promise<ClientResponse<AuthResponse>> {
        const response = await this.request("/auth/login", AuthResponseSchema, {
            method: "POST",
            body: JSON.stringify({ username, password }),
            skipAuth: true,
        });
        if (response.success) setCsrfToken(response.data.csrf_token);
        return response;
    }

    async setup(username: string, password: string, setupToken?: string): Promise<ClientResponse<AuthResponse>> {
        const response = await this.request("/auth/setup", AuthResponseSchema, {
            method: "POST",
            headers: setupToken ? { "X-Setup-Token": setupToken } : undefined,
            body: JSON.stringify({ username, password }),
            skipAuth: true,
        });
        if (response.success) setCsrfToken(response.data.csrf_token);
        return response;
    }

    async logout(): Promise<ClientResponse<z.infer<typeof SuccessResponseSchema>>> {
        const response = await this.request("/auth/logout", SuccessResponseSchema, { method: "POST" });
        setCsrfToken(null);
        return response;
    }

    async checkAuthStatus(): Promise<ClientResponse<SetupStatus>> {
        return this.request("/auth/status", SetupStatusSchema, { skipAuth: true });
    }

    async me(): Promise<ClientResponse<AuthResponse>> {
        const response = await this.request("/auth/me", AuthResponseSchema);
        if (response.success) setCsrfToken(response.data.csrf_token);
        return response;
    }

    async changePassword(currentPassword: string, newPassword: string): Promise<ClientResponse<z.infer<typeof SuccessResponseSchema>>> {
        const response = await this.request("/auth/password", SuccessResponseSchema, {
            method: "PUT",
            body: JSON.stringify({ current_password: currentPassword, new_password: newPassword }),
        });
        if (response.success) setCsrfToken(null);
        return response;
    }

    async updatePreferences(input: { language?: "fr" | "en"; accent_color?: string }): Promise<ClientResponse<UserInfo>> {
        return this.request("/auth/preferences", UserInfoSchema, {
            method: "PATCH",
            body: JSON.stringify(input),
        });
    }

    async sessions(): Promise<ClientResponse<SessionInfo[]>> {
        return this.request("/auth/sessions", z.array(SessionInfoSchema));
    }

    async revokeSession(id: string): Promise<ClientResponse<z.infer<typeof SuccessResponseSchema>>> {
        return this.request(`/auth/sessions/${encodeURIComponent(id)}`, SuccessResponseSchema, {
            method: "DELETE",
        });
    }

    async revokeOtherSessions(): Promise<ClientResponse<z.infer<typeof SuccessResponseSchema>>> {
        return this.request("/auth/sessions/revoke-others", SuccessResponseSchema, { method: "POST" });
    }
}
