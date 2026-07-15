import { ChangeEvent, useRef, useState } from "react";
import { Archive, ShieldCheck, Upload, X } from "lucide-react";
import { Button } from "@/components/ui";
import { useLanguage } from "@/contexts/LanguageContext";
import { useToast } from "@/contexts/ToastContext";
import { BedrockArchiveAuthorization } from "@/schemas/operations";
import { Job } from "@/schemas/api";
import { apiService } from "@/services";
import { ImportUploadTask } from "@/services/api/imports.client";
import { formatBytes } from "@/utils/formatters";

const SHA256_PATTERN = /^[0-9a-f]{64}$/i;

function isZipSignature(bytes: Uint8Array): boolean {
    return bytes.length >= 4
        && bytes[0] === 0x50
        && bytes[1] === 0x4b
        && ((bytes[2] === 0x03 && bytes[3] === 0x04)
            || (bytes[2] === 0x05 && bytes[3] === 0x06)
            || (bytes[2] === 0x07 && bytes[3] === 0x08));
}

export default function BedrockArchiveUploadNotice({
    authorization,
    canUpload,
    onAccepted,
}: {
    authorization: BedrockArchiveAuthorization;
    canUpload: boolean;
    onAccepted: (job: Job) => void;
}) {
    const { t } = useLanguage();
    const toast = useToast();
    const inputRef = useRef<HTMLInputElement>(null);
    const taskRef = useRef<ImportUploadTask | null>(null);
    const cancelledRef = useRef(false);
    const [file, setFile] = useState<File | null>(null);
    const [sha256, setSha256] = useState("");
    const [uploading, setUploading] = useState(false);
    const [progress, setProgress] = useState(0);

    const errorText = (value: string) => {
        const translated = t(value);
        return translated === value ? value : translated;
    };

    const selectFile = async (event: ChangeEvent<HTMLInputElement>) => {
        const selected = event.target.files?.[0] ?? null;
        event.target.value = "";
        if (!selected) return;
        if (selected.size === 0 || selected.size > authorization.interaction.max_bytes) {
            toast.error(t("bedrock_upload.invalid_size"));
            return;
        }
        const signature = new Uint8Array(await selected.slice(0, 4).arrayBuffer());
        if (!selected.name.toLowerCase().endsWith(".zip") || !isZipSignature(signature)) {
            toast.error(t("bedrock_upload.invalid_archive"));
            return;
        }
        setFile(selected);
        setProgress(0);
    };

    const upload = async () => {
        if (!file || !SHA256_PATTERN.test(sha256)) {
            toast.error(t("bedrock_upload.sha256_invalid"));
            return;
        }
        cancelledRef.current = false;
        setUploading(true);
        const task = apiService.imports.uploadZip(authorization.interaction.instance_id, file, {
            idempotencyKey: authorization.job_id,
            sha256: sha256.toLowerCase(),
            onProgress: (value) => setProgress(value.percent),
        });
        taskRef.current = task;
        const response = await task.response;
        taskRef.current = null;
        setUploading(false);
        if (cancelledRef.current) return;
        if (!response.success) {
            toast.error(errorText(response.error.message));
            return;
        }
        toast.success(t("bedrock_upload.accepted"));
        onAccepted(response.data);
    };

    const cancel = () => {
        cancelledRef.current = true;
        taskRef.current?.cancel();
        taskRef.current = null;
        setUploading(false);
        setProgress(0);
        toast.info(t("bedrock_upload.cancelled"));
    };

    return (
        <section className="bedrock-archive-upload" role="status" aria-live="polite" aria-labelledby="bedrock-upload-title">
            <span className="bedrock-archive-upload__icon"><Archive size={22} aria-hidden="true" /></span>
            <div className="bedrock-archive-upload__content">
                <h2 id="bedrock-upload-title">{t("bedrock_upload.title")}</h2>
                <p>{t("bedrock_upload.description")}</p>
                {authorization.interaction.version && (
                    <p className="helper-text">{t("bedrock_upload.expected_version")} <strong>{authorization.interaction.version}</strong></p>
                )}
                {!canUpload ? (
                    <p className="operations-warning">{t("bedrock_upload.owner_only")}</p>
                ) : (
                    <div className="bedrock-archive-upload__form">
                        <input
                            ref={inputRef}
                            className="sr-only"
                            type="file"
                            accept=".zip,application/zip"
                            aria-label={t("bedrock_upload.file_label")}
                            onChange={(event) => void selectFile(event)}
                        />
                        <Button type="button" variant="secondary" disabled={uploading} icon={<Archive size={16} aria-hidden="true" />} onClick={() => inputRef.current?.click()}>
                            {file ? t("bedrock_upload.replace_file") : t("bedrock_upload.choose_file")}
                        </Button>
                        {file && <span className="bedrock-archive-upload__filename">{file.name} · {formatBytes(file.size)}</span>}
                        <label htmlFor="bedrock-archive-sha256">{t("bedrock_upload.sha256")}</label>
                        <input
                            id="bedrock-archive-sha256"
                            className="input bedrock-archive-upload__sha"
                            value={sha256}
                            disabled={uploading}
                            inputMode="text"
                            autoComplete="off"
                            spellCheck={false}
                            minLength={64}
                            maxLength={64}
                            pattern="[0-9A-Fa-f]{64}"
                            placeholder={"a".repeat(64)}
                            onChange={(event) => setSha256(event.target.value.trim())}
                        />
                        <p className="helper-text"><ShieldCheck size={14} aria-hidden="true" />{t("bedrock_upload.security_hint")} {formatBytes(authorization.interaction.max_bytes)}.</p>
                        {uploading && (
                            <div className="bedrock-archive-upload__progress">
                                <progress max={100} value={progress} aria-label={t("bedrock_upload.progress")} />
                                <span>{Math.round(progress)} %</span>
                            </div>
                        )}
                        <div className="bedrock-archive-upload__actions">
                            {uploading
                                ? <Button type="button" variant="secondary" icon={<X size={16} aria-hidden="true" />} onClick={cancel}>{t("common.cancel")}</Button>
                                : <Button type="button" icon={<Upload size={16} aria-hidden="true" />} disabled={!file || !SHA256_PATTERN.test(sha256)} onClick={() => void upload()}>{t("bedrock_upload.upload")}</Button>}
                        </div>
                    </div>
                )}
            </div>
        </section>
    );
}
