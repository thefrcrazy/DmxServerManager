import { CloudDownload, FileArchive, LoaderCircle, PackagePlus, ShieldCheck, Trash2, Upload, X } from "lucide-react";
import { ChangeEvent, FormEvent, useCallback, useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui";
import { useDialog } from "@/contexts/DialogContext";
import { useLanguage } from "@/contexts/LanguageContext";
import { useToast } from "@/contexts/ToastContext";
import { InstalledMod } from "@/schemas/operations";
import { apiService } from "@/services";
import { ModProvider } from "@/services/api/mods.client";
import { formatBytes } from "@/utils/formatters";

const MAX_MOD_BYTES = 512 * 1_024 * 1_024;

interface ServerModsProps {
    instanceId: string;
    isInstalled: boolean;
    isStopped: boolean;
    refreshSignal?: number;
}

function validJarName(name: string): boolean {
    return name.length > 4
        && name.length <= 255
        && name.toLocaleLowerCase("en-US").endsWith(".jar")
        && !name.includes("/")
        && !name.includes("\\")
        && !name.includes(":")
        && !/[\u0000-\u001f\u007f]/.test(name);
}

async function hasZipSignature(file: File): Promise<boolean> {
    const signature = new Uint8Array(await file.slice(0, 4).arrayBuffer());
    return signature.length === 4
        && signature[0] === 0x50
        && signature[1] === 0x4b
        && signature[2] === 0x03
        && signature[3] === 0x04;
}

export default function ServerMods({ instanceId, isInstalled, isStopped, refreshSignal = 0 }: ServerModsProps) {
    const { t, language } = useLanguage();
    const toast = useToast();
    const { confirm } = useDialog();
    const inputRef = useRef<HTMLInputElement>(null);
    const cancelUploadRef = useRef<(() => void) | null>(null);
    const mountedRef = useRef(true);
    const cancelledRef = useRef(false);
    const [mods, setMods] = useState<InstalledMod[]>([]);
    const [selectedFile, setSelectedFile] = useState<File | null>(null);
    const [progress, setProgress] = useState(0);
    const [uploading, setUploading] = useState(false);
    const [provider, setProvider] = useState<ModProvider>("modrinth");
    const [providerProjectId, setProviderProjectId] = useState("");
    const [providerVersionId, setProviderVersionId] = useState("");
    const [installingProvider, setInstallingProvider] = useState(false);
    const [deletingId, setDeletingId] = useState<string | null>(null);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const canMutate = isInstalled && isStopped;

    const load = useCallback(async () => {
        const response = await apiService.mods.list(instanceId);
        if (!mountedRef.current) return;
        setLoading(false);
        if (!response.success) {
            setError(response.error.message);
            return;
        }
        setMods(response.data);
        setError(null);
    }, [instanceId]);

    useEffect(() => { void load(); }, [load, refreshSignal]);
    useEffect(() => {
        mountedRef.current = true;
        return () => {
            mountedRef.current = false;
            cancelUploadRef.current?.();
        };
    }, []);

    const selectFile = (event: ChangeEvent<HTMLInputElement>) => {
        const file = event.target.files?.[0] ?? null;
        event.target.value = "";
        if (!file) return;
        if (!validJarName(file.name)) return toast.error(t("server_detail.mods.invalid_filename"));
        if (file.size === 0 || file.size > MAX_MOD_BYTES) return toast.error(t("server_detail.mods.invalid_size"));
        setSelectedFile(file);
        setProgress(0);
    };

    const upload = async () => {
        if (!selectedFile || !canMutate || uploading) return;
        if (!(await hasZipSignature(selectedFile))) return toast.error(t("server_detail.mods.invalid_archive"));
        setUploading(true);
        setProgress(0);
        cancelledRef.current = false;
        const task = apiService.mods.uploadManual(instanceId, selectedFile, ({ percent }) => {
            if (mountedRef.current) setProgress(percent);
        });
        cancelUploadRef.current = task.cancel;
        const response = await task.response;
        cancelUploadRef.current = null;
        if (!mountedRef.current) return;
        setUploading(false);
        if (cancelledRef.current) {
            setProgress(0);
            return;
        }
        if (!response.success) return toast.error(response.error.message);
        setMods((current) => [response.data, ...current.filter((item) => item.id !== response.data.id)]);
        setSelectedFile(null);
        setProgress(0);
        toast.success(t("server_detail.mods.uploaded"));
    };

    const cancelUpload = () => {
        cancelledRef.current = true;
        cancelUploadRef.current?.();
        cancelUploadRef.current = null;
        setUploading(false);
        setProgress(0);
    };

    const remove = async (mod: InstalledMod) => {
        const accepted = await confirm(`${t("server_detail.mods.delete_confirm")} « ${mod.display_name} » ?`, { isDestructive: true });
        if (!accepted) return;
        setDeletingId(mod.id);
        const response = await apiService.mods.remove(instanceId, mod.id);
        setDeletingId(null);
        if (!response.success) return toast.error(response.error.message);
        setMods((current) => current.filter((item) => item.id !== mod.id));
        toast.success(t("server_detail.mods.deleted"));
    };

    const installFromProvider = async (event: FormEvent<HTMLFormElement>) => {
        event.preventDefault();
        if (!canMutate || installingProvider || !validProviderId(provider, providerProjectId) || !validProviderId(provider, providerVersionId)) {
            return toast.error(t("server_detail.mods.invalid_provider_id"));
        }
        setInstallingProvider(true);
        const response = await apiService.mods.installProvider(instanceId, {
            provider,
            project_id: providerProjectId,
            version_id: providerVersionId,
        });
        if (!mountedRef.current) return;
        setInstallingProvider(false);
        if (!response.success) return toast.error(response.error.message);
        setMods((current) => [response.data, ...current.filter((item) => item.id !== response.data.id)]);
        setProviderProjectId("");
        setProviderVersionId("");
        toast.success(t("server_detail.mods.provider_installed"));
    };

    return (
        <section className="server-mods card" aria-labelledby="mods-heading">
            <div className="server-mods__header">
                <div>
                    <h2 id="mods-heading">{t("server_detail.mods.title")}</h2>
                    <p>{t("server_detail.mods.subtitle")}</p>
                </div>
                <Button icon={<PackagePlus size={17} aria-hidden="true" />} disabled={!canMutate || uploading} onClick={() => inputRef.current?.click()}>
                    {t("server_detail.mods.choose_jar")}
                </Button>
                <input
                    ref={inputRef}
                    className="sr-only"
                    type="file"
                    accept=".jar,application/java-archive,application/zip,application/octet-stream"
                    aria-label={t("server_detail.mods.file_label")}
                    onChange={selectFile}
                />
            </div>
            {!canMutate && <p className="operations-warning" role="status">{t("server_detail.mods.install_stop_required")}</p>}
            <p className="server-mods__security"><ShieldCheck size={16} aria-hidden="true" />{t("server_detail.mods.security_hint")}</p>
            <form className="mod-provider" onSubmit={(event) => void installFromProvider(event)}>
                <div className="mod-provider__heading">
                    <CloudDownload size={20} aria-hidden="true" />
                    <div><h3>{t("server_detail.mods.provider_title")}</h3><p>{t("server_detail.mods.provider_hint")}</p></div>
                </div>
                <div className="mod-provider__fields">
                    <label className="form-group">
                        <span>{t("server_detail.mods.provider")}</span>
                        <select value={provider} onChange={(event) => setProvider(event.target.value as ModProvider)} disabled={installingProvider}>
                            <option value="modrinth">Modrinth</option>
                            <option value="curseforge">CurseForge</option>
                        </select>
                    </label>
                    <label className="form-group">
                        <span>{t("server_detail.mods.project_id")}</span>
                        <input value={providerProjectId} onChange={(event) => setProviderProjectId(event.target.value)} autoComplete="off" maxLength={64} required disabled={installingProvider} />
                    </label>
                    <label className="form-group">
                        <span>{t("server_detail.mods.version_id")}</span>
                        <input value={providerVersionId} onChange={(event) => setProviderVersionId(event.target.value)} autoComplete="off" maxLength={64} required disabled={installingProvider} />
                    </label>
                    <Button type="submit" icon={<CloudDownload size={16} aria-hidden="true" />} isLoading={installingProvider} disabled={!canMutate || uploading}>
                        {t("server_detail.mods.install_provider")}
                    </Button>
                </div>
            </form>
            {selectedFile && (
                <div className="mod-upload" aria-live="polite">
                    <FileArchive size={24} aria-hidden="true" />
                    <div className="mod-upload__file"><strong>{selectedFile.name}</strong><span>{formatBytes(selectedFile.size)}</span></div>
                    {uploading ? <>
                        <div className="mod-upload__progress">
                            <progress max="100" value={progress} aria-label={t("server_detail.mods.upload_progress")} />
                            <span>{Math.round(progress)} %</span>
                        </div>
                        <Button size="sm" variant="secondary" icon={<X size={15} aria-hidden="true" />} onClick={cancelUpload}>{t("common.cancel")}</Button>
                    </> : <>
                        <Button size="sm" icon={<Upload size={15} aria-hidden="true" />} onClick={() => void upload()} disabled={!canMutate}>{t("server_detail.mods.import")}</Button>
                        <button type="button" className="icon-action" onClick={() => setSelectedFile(null)} aria-label={t("server_detail.mods.clear_selection")}><X size={15} aria-hidden="true" /></button>
                    </>}
                </div>
            )}
            {loading && <div className="operations-loading"><LoaderCircle className="spinner" aria-hidden="true" />{t("common.loading")}</div>}
            {error && <div className="operations-error" role="alert">{error}<Button size="sm" variant="secondary" onClick={() => void load()}>{t("administration.retry")}</Button></div>}
            {!loading && !error && mods.length === 0 && <div className="operations-empty"><FileArchive aria-hidden="true" /><p>{t("server_detail.mods.empty")}</p></div>}
            {!loading && !error && mods.length > 0 && <div className="table-scroll"><table className="mods-table">
                <thead><tr><th>{t("server_detail.mods.name")}</th><th>{t("server_detail.mods.source")}</th><th>{t("server_detail.mods.size")}</th><th>{t("server_detail.mods.checksum")}</th><th>{t("server_detail.mods.status")}</th><th>{t("server_detail.mods.added_at")}</th><th><span className="sr-only">{t("common.actions")}</span></th></tr></thead>
                <tbody>{mods.map((mod) => <tr key={mod.id}>
                    <td><strong>{mod.display_name}</strong></td>
                    <td>{mod.source === "manual" ? t("server_detail.mods.manual") : mod.source === "modrinth" ? "Modrinth" : mod.source === "curseforge" ? "CurseForge" : mod.source}</td>
                    <td>{formatBytes(mod.size_bytes)}</td>
                    <td><code title={mod.checksum_sha256}>{mod.checksum_sha256.slice(0, 12)}…</code></td>
                    <td><span className={`badge badge--${mod.enabled ? "success" : "muted"}`}>{mod.enabled ? t("common.active") : t("common.inactive")}</span></td>
                    <td><time dateTime={mod.created_at}>{new Date(mod.created_at).toLocaleString(language === "fr" ? "fr-FR" : "en-US")}</time></td>
                    <td><button type="button" className="icon-action icon-action--danger" disabled={!canMutate || deletingId === mod.id || uploading} onClick={() => void remove(mod)} aria-label={`${t("common.delete")} ${mod.display_name}`}><Trash2 size={15} aria-hidden="true" /></button></td>
                </tr>)}</tbody>
            </table></div>}
        </section>
    );
}

function validProviderId(provider: ModProvider, value: string): boolean {
    return provider === "modrinth"
        ? /^[A-Za-z0-9]{1,64}$/.test(value)
        : /^[1-9][0-9]{0,9}$/.test(value);
}
