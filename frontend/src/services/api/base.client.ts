// Base API client logic
export interface ApiResponse<T> {
    data: T;
    success: boolean;
    timestamp: string;
    error?: ApiError;
}

export interface ApiError {
    error: string;
    code?: string;
}

export const API_BASE_URL = "/api/v1";

export interface ApiOptions extends RequestInit {
    skipAuth?: boolean;
}

export class BaseClient {
    protected getToken(): string | null {
        return localStorage.getItem("token");
    }

    protected async request<T>(endpoint: string, options: ApiOptions = {}): Promise<ApiResponse<T>> {
        const { skipAuth = false, ...fetchOptions } = options;

        const headers: HeadersInit = {
            "Content-Type": "application/json",
            ...fetchOptions.headers,
        };

        if (!skipAuth) {
            const token = this.getToken();
            if (token) {
                (headers as Record<string, string>)["Authorization"] = `Bearer ${token}`;
            }
        }

        try {
            const response = await fetch(`${API_BASE_URL}${endpoint}`, {
                ...fetchOptions,
                headers,
            });

            const timestamp = new Date().toISOString();

            if (response.status === 401) {
                // Dispatch event for auth context to handle
                window.dispatchEvent(new CustomEvent("logout-required"));
                throw new Error("Session expired");
            }

            if (!response.ok) {
                const errorData = await response.json().catch(() => ({ error: "Unknown error" })) as ApiError;
                throw new Error(errorData.error || errorData.code || `HTTP ${response.status}`);
            }

            const text = await response.text();
            const data = text ? JSON.parse(text) : null;

            return {
                data,
                success: true,
                timestamp
            };
        } catch (error) {
            const timestamp = new Date().toISOString();
            if (error instanceof Error) {
                return {
                    data: null as unknown as T,
                    success: false,
                    timestamp,
                    error: {
                        error: error.message,
                        code: "API_ERROR"
                    }
                };
            }
            throw error;
        }
    }
}
