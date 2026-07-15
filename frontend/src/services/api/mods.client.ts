import { z } from "zod";
import { ProblemDetailsSchema } from "@/schemas/api";
import {
    InstalledMod,
    InstalledModListSchema,
    InstalledModSchema,
    ModProviderConfigurationSchema,
    ModProviderStatus,
    ModProviderStatusSchema,
    OperationSuccessSchema,
} from "@/schemas/operations";
import {
    API_BASE_URL,
    ApiClientError,
    BaseClient,
    ClientResponse,
    getCsrfToken,
    setCsrfToken,
} from "./base.client";
import { queryString } from "./query";

export interface ModUploadProgress {
    loaded: number;
    total: number;
    percent: number;
}

export interface ModUploadTask {
    response: Promise<ClientResponse<InstalledMod>>;
    cancel: () => void;
}

export type ModProvider = "modrinth" | "curseforge";

export interface ProviderModRequest {
    provider: ModProvider;
    project_id: string;
    version_id: string;
}

function failure(error: ApiClientError, timestamp: string): ClientResponse<never> {
    return { data: undefined as never, success: false, timestamp, error };
}

export class ModsClient extends BaseClient {
    providerStatus(): Promise<ClientResponse<ModProviderStatus>> {
        return this.request("/mods/providers", ModProviderStatusSchema);
    }

    configureCurseForge(apiKey: string): Promise<ClientResponse<{ configured: boolean }>> {
        return this.request("/mods/providers/curseforge", ModProviderConfigurationSchema, {
            method: "PUT",
            body: JSON.stringify({ api_key: apiKey }),
        });
    }

    clearCurseForge(): Promise<ClientResponse<{ configured: boolean }>> {
        return this.request("/mods/providers/curseforge", ModProviderConfigurationSchema, {
            method: "DELETE",
        });
    }

    list(instanceId: string): Promise<ClientResponse<InstalledMod[]>> {
        return this.request(
            `/servers/${encodeURIComponent(instanceId)}/mods`,
            InstalledModListSchema,
        ).then((response) => response.success ? { ...response, data: response.data.items } : response);
    }

    remove(instanceId: string, modId: string): Promise<ClientResponse<z.infer<typeof OperationSuccessSchema>>> {
        return this.request(
            `/servers/${encodeURIComponent(instanceId)}/mods/${encodeURIComponent(modId)}`,
            OperationSuccessSchema,
            { method: "DELETE" },
        );
    }

    installProvider(instanceId: string, body: ProviderModRequest): Promise<ClientResponse<InstalledMod>> {
        return this.request(
            `/servers/${encodeURIComponent(instanceId)}/mods/provider`,
            InstalledModSchema,
            { method: "POST", body: JSON.stringify(body) },
        );
    }

    manualUploadUrl(instanceId: string, filename: string): string {
        return `${API_BASE_URL}/servers/${encodeURIComponent(instanceId)}/mods/manual${queryString({ filename })}`;
    }

    uploadManual(
        instanceId: string,
        file: File,
        onProgress: (progress: ModUploadProgress) => void,
    ): ModUploadTask {
        const xhr = new XMLHttpRequest();
        const timestamp = new Date().toISOString();
        const response = new Promise<ClientResponse<InstalledMod>>((resolve) => {
            xhr.open("POST", this.manualUploadUrl(instanceId, file.name));
            xhr.withCredentials = true;
            xhr.setRequestHeader("Accept", "application/json");
            xhr.setRequestHeader("Content-Type", "application/java-archive");
            const csrfToken = getCsrfToken();
            if (csrfToken) xhr.setRequestHeader("X-CSRF-Token", csrfToken);

            xhr.upload.addEventListener("progress", (event) => {
                const total = event.lengthComputable && event.total > 0 ? event.total : file.size;
                const loaded = Math.min(event.loaded, total);
                onProgress({
                    loaded,
                    total,
                    percent: total > 0 ? Math.min(100, (loaded / total) * 100) : 0,
                });
            });
            xhr.addEventListener("load", () => {
                const refreshedCsrf = xhr.getResponseHeader("X-CSRF-Token");
                if (refreshedCsrf) setCsrfToken(refreshedCsrf);
                let payload: unknown;
                try { payload = xhr.responseText ? JSON.parse(xhr.responseText) as unknown : undefined; } catch { payload = undefined; }
                if (xhr.status >= 200 && xhr.status < 300) {
                    const parsed = InstalledModSchema.safeParse(payload);
                    if (parsed.success) {
                        onProgress({ loaded: file.size, total: file.size, percent: 100 });
                        resolve({ data: parsed.data, success: true, timestamp });
                    } else {
                        resolve(failure(new ApiClientError(`Réponse API invalide: ${z.prettifyError(parsed.error)}`, xhr.status), timestamp));
                    }
                    return;
                }
                const problem = ProblemDetailsSchema.safeParse(payload);
                const detail = problem.success ? problem.data.detail ?? problem.data.title : `HTTP ${xhr.status}`;
                if (xhr.status === 401) {
                    setCsrfToken(null);
                    window.dispatchEvent(new CustomEvent("dmx-auth-required"));
                }
                resolve(failure(new ApiClientError(detail, xhr.status, problem.success ? problem.data : undefined), timestamp));
            });
            xhr.addEventListener("error", () => resolve(failure(new ApiClientError("Erreur réseau pendant l’upload", 0), timestamp)));
            xhr.addEventListener("abort", () => resolve(failure(new ApiClientError("Upload annulé", 0), timestamp)));
            xhr.send(file);
        });
        return { response, cancel: () => xhr.abort() };
    }
}
