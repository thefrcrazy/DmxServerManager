import {
    AlertTriangle,
    Clock3,
    FileCode2,
    LoaderCircle,
    RefreshCw,
    Save,
    X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui";
import { useLanguage } from "@/contexts/LanguageContext";
import { useToast } from "@/contexts/ToastContext";
import type {
    ConfigFileCategory,
    ConfigFileDocument,
    ConfigFileSummary,
} from "@/schemas/operations";
import { apiService } from "@/services";
import { formatBytes, formatDate } from "@/utils/formatters";

interface ServerConfigFilesProps {
    instanceId: string;
    category: ConfigFileCategory;
    canWrite: boolean;
    isRunning: boolean;
    refreshSignal?: number;
}

const LIVE_REFRESH_MS = 5_000;

export default function ServerConfigFiles({
    instanceId,
    category,
    canWrite,
    isRunning,
    refreshSignal = 0,
}: ServerConfigFilesProps) {
    const { t, language } = useLanguage();
    const toast = useToast();
    const [files, setFiles] = useState<ConfigFileSummary[]>([]);
    const [selectedPath, setSelectedPath] = useState<string | null>(null);
    const [document, setDocument] = useState<ConfigFileDocument | null>(null);
    const [draft, setDraft] = useState("");
    const [loading, setLoading] = useState(true);
    const [loadingDocument, setLoadingDocument] = useState(false);
    const [saving, setSaving] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [sourceChanged, setSourceChanged] = useState(false);
    const [dirty, setDirty] = useState(false);
    const dirtyRef = useRef(false);
    const baseShaRef = useRef<string | null>(null);

    const setEditorDirty = (value: boolean) => {
        dirtyRef.current = value;
        setDirty(value);
    };

    const loadList = useCallback(async () => {
        const response = await apiService.config.list(instanceId);
        setLoading(false);
        if (!response.success) {
            setError(response.error.message);
            return;
        }
        const next = response.data.items.filter((file) => file.category === category);
        setFiles(next);
        setSelectedPath((current) => current && next.some((file) => file.path === current)
            ? current
            : next[0]?.path ?? null);
        setError(null);
    }, [category, instanceId]);

    const loadDocument = useCallback(async (path: string, preserveDraft: boolean) => {
        if (!preserveDraft) setLoadingDocument(true);
        const response = await apiService.config.read(instanceId, path);
        setLoadingDocument(false);
        if (!response.success) {
            setError(response.error.message);
            return;
        }
        setDocument(response.data);
        setFiles((current) => current.map((file) => file.path === path ? response.data.file : file));
        if (preserveDraft && dirtyRef.current) {
            setSourceChanged(response.data.file.sha256 !== baseShaRef.current);
        } else {
            setDraft(response.data.queued_content ?? response.data.content);
            baseShaRef.current = response.data.file.sha256;
            setEditorDirty(false);
            setSourceChanged(false);
        }
        setError(null);
    }, [instanceId]);

    useEffect(() => {
        setLoading(true);
        void loadList();
        const timer = window.setInterval(() => void loadList(), LIVE_REFRESH_MS);
        return () => window.clearInterval(timer);
    }, [loadList, refreshSignal]);

    useEffect(() => {
        setDocument(null);
        setDraft("");
        baseShaRef.current = null;
        setEditorDirty(false);
        setSourceChanged(false);
        if (!selectedPath) return;
        void loadDocument(selectedPath, false);
        const timer = window.setInterval(() => void loadDocument(selectedPath, true), LIVE_REFRESH_MS);
        return () => window.clearInterval(timer);
    }, [loadDocument, refreshSignal, selectedPath]);

    const selected = useMemo(
        () => files.find((file) => file.path === selectedPath) ?? document?.file ?? null,
        [document, files, selectedPath],
    );

    const selectFile = (path: string) => {
        if (dirtyRef.current && path !== selectedPath) {
            toast.error(t("server_detail.native_config.discard_first"));
            return;
        }
        setSelectedPath(path);
    };

    const queue = async () => {
        if (!selectedPath || !document) return;
        setSaving(true);
        const response = await apiService.config.queue(
            instanceId,
            selectedPath,
            draft,
            baseShaRef.current,
        );
        setSaving(false);
        if (!response.success) {
            if (response.error.status === 409) setSourceChanged(true);
            toast.error(response.error.message);
            return;
        }
        setDocument(response.data);
        setDraft(response.data.queued_content ?? response.data.content);
        baseShaRef.current = response.data.file.sha256;
        setEditorDirty(false);
        setSourceChanged(false);
        toast.success(t("server_detail.native_config.queued"));
        await loadList();
    };

    const cancel = async () => {
        if (!selectedPath) return;
        setSaving(true);
        const response = await apiService.config.cancel(instanceId, selectedPath);
        setSaving(false);
        if (!response.success) {
            toast.error(response.error.message);
            return;
        }
        toast.success(t("server_detail.native_config.cancelled"));
        await Promise.all([loadList(), loadDocument(selectedPath, false)]);
    };

    const locale = language === "fr" ? "fr-FR" : "en-US";
    const queued = selected?.queued_change;

    return (
        <section className={`native-config-section native-config-section--${category}`} aria-labelledby={`native-config-${category}`}>
            <header className="native-config-section__header">
                <div>
                    <h3 id={`native-config-${category}`}><FileCode2 size={18} aria-hidden="true" />{t(`server_detail.native_config.${category}_title`)}</h3>
                    <p>{t(`server_detail.native_config.${category}_description`)}</p>
                </div>
                <Button type="button" size="sm" variant="ghost" icon={<RefreshCw size={15} />} onClick={() => {
                    void loadList();
                    if (selectedPath) void loadDocument(selectedPath, true);
                }}>{t("common.refresh")}</Button>
            </header>

            <div className="native-config-apply-hint" role="note">
                <Clock3 size={16} aria-hidden="true" />
                <span>{t(isRunning
                    ? "server_detail.native_config.running_hint"
                    : "server_detail.native_config.stopped_hint")}</span>
            </div>

            {loading && <div className="operations-loading"><LoaderCircle className="spinner" aria-hidden="true" />{t("common.loading")}</div>}
            {error && <div className="operations-error" role="alert">{error}</div>}
            {!loading && !error && files.length === 0 && <div className="operations-empty"><FileCode2 aria-hidden="true" /><p>{t("server_detail.native_config.empty")}</p></div>}

            {!loading && files.length > 0 && (
                <div className="native-config-workspace">
                    <nav className="native-config-files" aria-label={t(`server_detail.native_config.${category}_title`)}>
                        {files.map((file) => (
                            <button
                                key={file.path}
                                type="button"
                                className={`native-config-file ${file.path === selectedPath ? "native-config-file--active" : ""}`}
                                onClick={() => selectFile(file.path)}
                            >
                                <span className="native-config-file__name">{file.path.split("/").at(-1)}</span>
                                <span className="native-config-file__path">{file.path}</span>
                                <span className="native-config-file__meta">
                                    <span className={`badge badge--${file.exists ? "success" : "muted"}`}>{t(file.exists ? "server_detail.native_config.live" : "server_detail.native_config.missing")}</span>
                                    {file.queued_change && <span className={`badge badge--${file.queued_change.status === "pending" ? "warning" : "danger"}`}>{t(`server_detail.native_config.status.${file.queued_change.status}`)}</span>}
                                </span>
                            </button>
                        ))}
                    </nav>

                    <div className="native-config-editor">
                        {loadingDocument || !document || !selected ? (
                            <div className="operations-loading"><LoaderCircle className="spinner" aria-hidden="true" />{t("common.loading")}</div>
                        ) : (
                            <>
                                <div className="native-config-editor__meta">
                                    <div><strong>{selected.path}</strong><span>{selected.format.toUpperCase()}</span></div>
                                    <div>
                                        <span>{formatBytes(selected.size_bytes)}</span>
                                        <span>{formatDate(selected.modified_at, locale)}</span>
                                        {selected.sha256 && <code title={selected.sha256}>{selected.sha256.slice(0, 12)}…</code>}
                                    </div>
                                </div>
                                {sourceChanged && (
                                    <div className="native-config-conflict" role="alert">
                                        <AlertTriangle size={17} aria-hidden="true" />
                                        <span>{t("server_detail.native_config.source_changed")}</span>
                                    </div>
                                )}
                                {queued && queued.status !== "pending" && (
                                    <div className="native-config-conflict" role="alert">
                                        <AlertTriangle size={17} aria-hidden="true" />
                                        <span>{t(`server_detail.native_config.status_detail.${queued.status}`)}</span>
                                    </div>
                                )}
                                <label className="sr-only" htmlFor={`native-config-editor-${category}`}>{selected.path}</label>
                                <textarea
                                    id={`native-config-editor-${category}`}
                                    className="native-config-textarea"
                                    value={draft}
                                    onChange={(event) => {
                                        setDraft(event.target.value);
                                        setEditorDirty(true);
                                    }}
                                    readOnly={!canWrite}
                                    spellCheck={false}
                                />
                                <div className="native-config-editor__actions">
                                    <span>{dirty ? t("server_detail.native_config.unsaved") : queued?.status === "pending" ? t("server_detail.native_config.pending") : t("server_detail.native_config.up_to_date")}</span>
                                    <div>
                                        {dirty && <Button type="button" size="sm" variant="ghost" icon={<RefreshCw size={15} />} onClick={() => void loadDocument(selected.path, false)}>{t("server_detail.native_config.discard")}</Button>}
                                        {queued?.status === "pending" && canWrite && <Button type="button" size="sm" variant="secondary" icon={<X size={15} />} disabled={saving} onClick={() => void cancel()}>{t("server_detail.native_config.cancel_queue")}</Button>}
                                        {canWrite && <Button type="button" size="sm" icon={<Save size={15} />} disabled={!dirty || sourceChanged} isLoading={saving} onClick={() => void queue()}>{t("server_detail.native_config.queue")}</Button>}
                                    </div>
                                </div>
                            </>
                        )}
                    </div>
                </div>
            )}
        </section>
    );
}
