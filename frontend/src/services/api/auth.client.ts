import { BaseClient, ApiResponse } from "./base.client";
import { AuthResponse, SetupStatus } from "../../schemas/api";

export class AuthClient extends BaseClient {
    async login(username: string, password: string): Promise<ApiResponse<AuthResponse>> {
        return this.request<AuthResponse>("/auth/login", {
            method: "POST",
            body: JSON.stringify({ username, password }),
            skipAuth: true,
        });
    }

    async register(username: string, password: string): Promise<ApiResponse<AuthResponse>> {
        return this.request<AuthResponse>("/auth/register", {
            method: "POST",
            body: JSON.stringify({ username, password }),
            skipAuth: true,
        });
    }

    async checkAuthStatus(): Promise<ApiResponse<SetupStatus>> {
        return this.request<SetupStatus>("/auth/status", { skipAuth: true });
    }

    async me(): Promise<ApiResponse<AuthResponse["user"]>> {
        return this.request<AuthResponse["user"]>("/auth/me");
    }
}
