import { z } from "zod";
import { Job, JobSchema, ProblemDetailsSchema } from "@/schemas/api";
import {
    API_BASE_URL,
    ApiClientError,
    BaseClient,
    ClientResponse,
    getCsrfToken,
    setCsrfToken,
} from "./base.client";

export interface ImportUploadProgress {
    loaded: number;
    total: number;
    percent: number;
}

export interface ImportUploadTask {
    response: Promise<ClientResponse<Job>>;
    cancel: () => void;
}

function failed(error: ApiClientError, timestamp: string): ClientResponse<never> {
    return { data: undefined as never, success: false, timestamp, error };
}

function importPath(instanceId: string, mode: "copy" | "attach" | "zip"): string {
    return `/servers/${encodeURIComponent(instanceId)}/imports/${mode}`;
}

export class ImportsClient extends BaseClient {
    copy(instanceId: string, sourcePath: string, idempotencyKey: string): Promise<ClientResponse<Job>> {
        return this.sourceImport(instanceId, "copy", sourcePath, idempotencyKey);
    }

    attach(instanceId: string, sourcePath: string, idempotencyKey: string): Promise<ClientResponse<Job>> {
        return this.sourceImport(instanceId, "attach", sourcePath, idempotencyKey);
    }

    private sourceImport(
        instanceId: string,
        mode: "copy" | "attach",
        sourcePath: string,
        idempotencyKey: string,
    ): Promise<ClientResponse<Job>> {
        return this.request(importPath(instanceId, mode), JobSchema, {
            method: "POST",
            headers: { "Idempotency-Key": idempotencyKey },
            body: JSON.stringify({ source_path: sourcePath }),
        });
    }

    uploadZip(
        instanceId: string,
        file: File,
        options: {
            idempotencyKey: string;
            sha256?: string;
            onProgress: (progress: ImportUploadProgress) => void;
        },
    ): ImportUploadTask {
        const xhr = new XMLHttpRequest();
        const timestamp = new Date().toISOString();
        const response = new Promise<ClientResponse<Job>>((resolve) => {
            xhr.open("POST", `${API_BASE_URL}${importPath(instanceId, "zip")}`);
            xhr.withCredentials = true;
            xhr.setRequestHeader("Accept", "application/json");
            xhr.setRequestHeader("Content-Type", "application/zip");
            xhr.setRequestHeader("Idempotency-Key", options.idempotencyKey);
            if (options.sha256) xhr.setRequestHeader("X-Dmx-Archive-Sha256", options.sha256);
            const csrfToken = getCsrfToken();
            if (csrfToken) xhr.setRequestHeader("X-CSRF-Token", csrfToken);

            xhr.upload.addEventListener("progress", (event) => {
                const total = event.lengthComputable && event.total > 0 ? event.total : file.size;
                const loaded = Math.min(event.loaded, total);
                options.onProgress({
                    loaded,
                    total,
                    percent: total > 0 ? Math.min(100, (loaded / total) * 100) : 0,
                });
            });
            xhr.addEventListener("load", () => {
                const refreshedCsrf = xhr.getResponseHeader("X-CSRF-Token");
                if (refreshedCsrf) setCsrfToken(refreshedCsrf);
                let payload: unknown;
                try {
                    payload = xhr.responseText ? JSON.parse(xhr.responseText) as unknown : undefined;
                } catch {
                    payload = undefined;
                }
                if (xhr.status >= 200 && xhr.status < 300) {
                    const parsed = JobSchema.safeParse(payload);
                    if (parsed.success) {
                        options.onProgress({ loaded: file.size, total: file.size, percent: 100 });
                        resolve({ data: parsed.data, success: true, timestamp });
                    } else {
                        resolve(failed(
                            new ApiClientError(`Réponse API invalide: ${z.prettifyError(parsed.error)}`, xhr.status),
                            timestamp,
                        ));
                    }
                    return;
                }
                const problem = ProblemDetailsSchema.safeParse(payload);
                const detail = problem.success
                    ? problem.data.detail ?? problem.data.title
                    : `HTTP ${xhr.status}`;
                if (xhr.status === 401) {
                    setCsrfToken(null);
                    window.dispatchEvent(new CustomEvent("dmx-auth-required"));
                }
                resolve(failed(
                    new ApiClientError(detail, xhr.status, problem.success ? problem.data : undefined),
                    timestamp,
                ));
            });
            xhr.addEventListener("error", () => resolve(failed(new ApiClientError("imports.network_error", 0), timestamp)));
            xhr.addEventListener("abort", () => resolve(failed(new ApiClientError("imports.upload_cancelled", 0), timestamp)));
            xhr.send(file);
        });
        return { response, cancel: () => xhr.abort() };
    }
}
