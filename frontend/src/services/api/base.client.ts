import { z } from "zod";
import { ProblemDetails, ProblemDetailsSchema } from "@/schemas/api";

export interface ApiResponse<T> {
    data: T;
    success: true;
    timestamp: string;
    error?: never;
}

export interface FailedApiResponse {
    data: never;
    success: false;
    timestamp: string;
    error: ApiClientError;
}

export type ClientResponse<T> = ApiResponse<T> | FailedApiResponse;

export const API_BASE_URL = "/api/v1";

const SAFE_METHODS = new Set(["GET", "HEAD", "OPTIONS"]);
let csrfToken: string | null = null;

export function setCsrfToken(token: string | null): void {
    csrfToken = token;
}

export function getCsrfToken(): string | null {
    return csrfToken;
}

export interface ApiFetchOptions extends RequestInit {
    suppressAuthEvent?: boolean;
}

export async function apiFetch(input: string, options: ApiFetchOptions = {}): Promise<Response> {
    const validationBase = new URL("http://dmx.invalid");
    const target = new URL(input, validationBase);
    if (target.origin !== validationBase.origin || (target.pathname !== API_BASE_URL && !target.pathname.startsWith(`${API_BASE_URL}/`))) {
        throw new ApiClientError("URL API hors périmètre", 0);
    }
    const { suppressAuthEvent = false, ...requestOptions } = options;
    const method = (requestOptions.method ?? "GET").toUpperCase();
    const headers = new Headers(requestOptions.headers);
    headers.delete("Authorization");

    if (!SAFE_METHODS.has(method) && csrfToken) {
        headers.set("X-CSRF-Token", csrfToken);
    }
    if (typeof requestOptions.body === "string" && !headers.has("Content-Type")) {
        headers.set("Content-Type", "application/json");
    }

    const response = await fetch(input, {
        ...requestOptions,
        credentials: "same-origin",
        headers,
    });

    const refreshedCsrf = response.headers.get("X-CSRF-Token");
    if (refreshedCsrf) setCsrfToken(refreshedCsrf);
    if (response.status === 401 && !suppressAuthEvent) {
        setCsrfToken(null);
        window.dispatchEvent(new CustomEvent("dmx-auth-required"));
    }
    return response;
}

export class ApiClientError extends Error {
    readonly status: number;
    readonly code?: string;
    readonly traceId?: string;
    readonly problem?: ProblemDetails;

    constructor(message: string, status: number, problem?: ProblemDetails) {
        super(message);
        this.name = "ApiClientError";
        this.status = status;
        this.code = problem?.code;
        this.traceId = problem?.trace_id;
        this.problem = problem;
    }
}

async function responseError(response: Response): Promise<ApiClientError> {
    const contentType = response.headers.get("content-type") ?? "";
    const payload = await response.json().catch(() => null) as unknown;
    if (contentType.includes("application/problem+json")) {
        const parsed = ProblemDetailsSchema.safeParse(payload);
        if (parsed.success) {
            return new ApiClientError(parsed.data.detail ?? parsed.data.title, response.status, parsed.data);
        }
    }
    const fallback = z.object({ title: z.string().optional(), detail: z.string().optional(), message: z.string().optional() }).safeParse(payload);
    const message = fallback.success
        ? fallback.data.detail ?? fallback.data.title ?? fallback.data.message ?? `HTTP ${response.status}`
        : `HTTP ${response.status}`;
    return new ApiClientError(message, response.status);
}

export interface ApiOptions extends ApiFetchOptions {
    skipAuth?: boolean;
}

export class BaseClient {
    protected async request<T>(endpoint: string, schema: z.ZodType<T>, options: ApiOptions = {}): Promise<ClientResponse<T>> {
        const { skipAuth = false, ...fetchOptions } = options;
        const timestamp = new Date().toISOString();

        try {
            const response = await apiFetch(`${API_BASE_URL}${endpoint}`, {
                ...fetchOptions,
                suppressAuthEvent: skipAuth,
            });
            if (!response.ok) throw await responseError(response);

            const text = await response.text();
            const payload = text ? JSON.parse(text) as unknown : undefined;
            const parsed = schema.safeParse(payload);
            if (!parsed.success) {
                throw new ApiClientError(
                    `Réponse API invalide: ${z.prettifyError(parsed.error)}`,
                    response.status,
                );
            }
            return { data: parsed.data, success: true, timestamp };
        } catch (cause) {
            const error = cause instanceof ApiClientError
                ? cause
                : new ApiClientError(cause instanceof Error ? cause.message : "Erreur API inconnue", 0);
            if (error.code === "AUTH_009" && typeof window !== "undefined") {
                window.dispatchEvent(new CustomEvent("dmx-password-change-required"));
            }
            return { data: undefined as never, success: false, timestamp, error };
        }
    }
}
