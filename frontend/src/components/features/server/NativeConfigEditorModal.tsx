import { DiffEditor, Editor } from "@monaco-editor/react";
import { AlertTriangle, Columns2, FileCode2, RefreshCw, Save, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent } from "react";
import { Button } from "@/components/ui";
import { useDialog } from "@/contexts/DialogContext";
import { useLanguage } from "@/contexts/LanguageContext";
import { useToast } from "@/contexts/ToastContext";
import type { ConfigFileDocument, ConfigFileSummary } from "@/schemas/operations";
import { apiService } from "@/services";
import { formatBytes, formatDate } from "@/utils/formatters";

interface NativeConfigEditorModalProps {
    instanceId: string;
    file: ConfigFileSummary;
    canWrite: boolean;
    isRunning: boolean;
    onClose: () => void;
    onChanged: () => void;
}

const LIVE_REFRESH_MS = 5_000;

function monacoLanguage(format: ConfigFileSummary["format"]): string {
    if (format === "properties" || format === "ini") return "ini";
    if (format === "text") return "plaintext";
    return format;
}

export default function NativeConfigEditorModal({
    instanceId,
    file,
    canWrite,
    isRunning,
    onClose,
    onChanged,
}: NativeConfigEditorModalProps) {
    const { t, language } = useLanguage();
    const toast = useToast();
    const { confirm } = useDialog();
    const dialogRef = useRef<HTMLDivElement>(null);
    const previousFocusRef = useRef<HTMLElement | null>(null);
    const dirtyRef = useRef(false);
    const baseShaRef = useRef<string | null>(file.sha256);
    const [fileDocument, setFileDocument] = useState<ConfigFileDocument | null>(null);
    const [draft, setDraft] = useState("");
    const [dirty, setDirty] = useState(false);
    const [loading, setLoading] = useState(true);
    const [saving, setSaving] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [sourceChanged, setSourceChanged] = useState(false);
    const [view, setView] = useState<"editor" | "diff">("editor");

    const setEditorDirty = useCallback((value: boolean) => {
        dirtyRef.current = value;
        setDirty(value);
    }, []);

    const loadDocument = useCallback(async (preserveDraft: boolean) => {
        if (!preserveDraft) setLoading(true);
        const response = await apiService.config.read(instanceId, file.path);
        setLoading(false);
        if (!response.success) {
            setError(response.error.message);
            return;
        }
        setFileDocument(response.data);
        if (preserveDraft && dirtyRef.current) {
            setSourceChanged(response.data.file.sha256 !== baseShaRef.current);
        } else {
            setDraft(response.data.queued_content ?? response.data.content);
            baseShaRef.current = response.data.file.sha256;
            setEditorDirty(false);
            setSourceChanged(false);
        }
        setError(null);
    }, [file.path, instanceId, setEditorDirty]);

    useEffect(() => {
        void loadDocument(false);
        const timer = window.setInterval(() => void loadDocument(true), LIVE_REFRESH_MS);
        return () => window.clearInterval(timer);
    }, [loadDocument]);

    const requestClose = useCallback(async () => {
        if (dirtyRef.current) {
            const accepted = await confirm(t("server_detail.native_config.close_confirm"), {
                title: t("server_detail.native_config.close_title"),
                confirmLabel: t("server_detail.native_config.close_without_saving"),
                isDestructive: true,
            });
            if (!accepted) return;
        }
        onClose();
    }, [confirm, onClose, t]);

    useEffect(() => {
        previousFocusRef.current = document.activeElement as HTMLElement | null;
        requestAnimationFrame(() => dialogRef.current?.focus());
        const handleBeforeUnload = (event: BeforeUnloadEvent) => {
            if (!dirtyRef.current) return;
            event.preventDefault();
        };
        window.addEventListener("beforeunload", handleBeforeUnload);
        return () => {
            window.removeEventListener("beforeunload", handleBeforeUnload);
            previousFocusRef.current?.focus();
        };
    }, []);

    const handleDialogKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
        if (event.key === "Escape") {
            event.preventDefault();
            void requestClose();
            return;
        }
        if (event.key !== "Tab") return;
        const focusable = [...event.currentTarget.querySelectorAll<HTMLElement>(
            "button:not([disabled]), a[href], input:not([disabled]), [tabindex]:not([tabindex='-1'])",
        )].filter((element) => !element.hasAttribute("aria-hidden"));
        if (focusable.length === 0) return;
        const first = focusable[0];
        const last = focusable.at(-1)!;
        if (event.shiftKey && document.activeElement === first) {
            event.preventDefault();
            last.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
            event.preventDefault();
            first.focus();
        }
    };

    const jsonError = useMemo(() => {
        if (file.format !== "json") return null;
        try {
            JSON.parse(draft);
            return null;
        } catch (parseError) {
            return parseError instanceof Error ? parseError.message : t("server_detail.native_config.invalid_json");
        }
    }, [draft, file.format, t]);

    const queue = async () => {
        if (!fileDocument || jsonError) return;
        setSaving(true);
        const response = await apiService.config.queue(
            instanceId,
            file.path,
            draft,
            baseShaRef.current,
        );
        setSaving(false);
        if (!response.success) {
            if (response.error.status === 409) setSourceChanged(true);
            toast.error(response.error.message);
            return;
        }
        setFileDocument(response.data);
        setDraft(response.data.queued_content ?? response.data.content);
        baseShaRef.current = response.data.file.sha256;
        setEditorDirty(false);
        setSourceChanged(false);
        toast.success(t("server_detail.native_config.queued"));
        onChanged();
    };

    const cancelQueue = async () => {
        setSaving(true);
        const response = await apiService.config.cancel(instanceId, file.path);
        setSaving(false);
        if (!response.success) {
            toast.error(response.error.message);
            return;
        }
        toast.success(t("server_detail.native_config.cancelled"));
        onChanged();
        await loadDocument(false);
    };

    const selected = fileDocument?.file ?? file;
    const queued = selected.queued_change;
    const locale = language === "fr" ? "fr-FR" : "en-US";
    const editorOptions = {
        ariaLabel: t("server_detail.native_config.editor_aria").replace("{{file}}", file.path.split("/").at(-1) ?? file.path),
        accessibilitySupport: "auto" as const,
        automaticLayout: true,
        fontFamily: "var(--font-family-mono)",
        fontSize: 13,
        lineNumbers: "on" as const,
        minimap: { enabled: false },
        readOnly: !canWrite,
        renderWhitespace: "selection" as const,
        scrollBeyondLastLine: false,
        tabSize: 4,
        wordWrap: "on" as const,
    };

    return (
        <div className="native-config-modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && void requestClose()}>
            <div
                ref={dialogRef}
                className="native-config-modal"
                role="dialog"
                aria-modal="true"
                aria-labelledby="native-config-modal-title"
                tabIndex={-1}
                onKeyDown={handleDialogKeyDown}
            >
                <header className="native-config-modal__header">
                    <div>
                        <span className="native-config-modal__eyebrow"><FileCode2 size={15} aria-hidden="true" />{t("server_detail.native_config.advanced_editor")}</span>
                        <h2 id="native-config-modal-title">{file.path.split("/").at(-1)}</h2>
                        <code>{file.path}</code>
                    </div>
                    <Button type="button" variant="ghost" size="icon" aria-label={t("common.close")} onClick={() => void requestClose()}><X size={18} /></Button>
                </header>

                <div className="native-config-modal__toolbar">
                    <div className="native-config-modal__metadata">
                        <span>{selected.format.toUpperCase()}</span>
                        <span>{formatBytes(selected.size_bytes)}</span>
                        <span>{selected.modified_at ? formatDate(selected.modified_at, locale) : t("server_detail.native_config.never_modified")}</span>
                    </div>
                    <div className="native-config-modal__view-switch" role="group" aria-label={t("server_detail.native_config.view_mode")}>
                        <button type="button" className={view === "editor" ? "active" : ""} aria-pressed={view === "editor"} onClick={() => setView("editor")}><FileCode2 size={15} />{t("server_detail.native_config.editor")}</button>
                        <button type="button" className={view === "diff" ? "active" : ""} aria-pressed={view === "diff"} onClick={() => setView("diff")}><Columns2 size={15} />{t("server_detail.native_config.diff")}</button>
                    </div>
                </div>

                <div className="native-config-modal__alerts">
                    {sourceChanged && <div className="native-config-conflict" role="alert"><AlertTriangle size={17} /><span>{t("server_detail.native_config.source_changed")}</span></div>}
                    {jsonError && <div className="native-config-conflict" role="alert"><AlertTriangle size={17} /><span>{t("server_detail.native_config.invalid_json")}: {jsonError}</span></div>}
                    {queued && queued.status !== "pending" && <div className="native-config-conflict" role="alert"><AlertTriangle size={17} /><span>{t(`server_detail.native_config.status_detail.${queued.status}`)}</span></div>}
                </div>

                <div className="native-config-modal__editor" aria-busy={loading}>
                    {loading ? <div className="operations-loading"><span className="spinner" />{t("common.loading")}</div> : error ? <div className="operations-error" role="alert">{error}</div> : fileDocument && (view === "diff" ? (
                        <DiffEditor
                            original={fileDocument.content}
                            modified={draft}
                            language={monacoLanguage(file.format)}
                            theme="vs-dark"
                            options={{ ...editorOptions, originalEditable: false, readOnly: true }}
                        />
                    ) : (
                        <Editor
                            value={draft}
                            language={monacoLanguage(file.format)}
                            theme="vs-dark"
                            options={editorOptions}
                            onChange={(value) => {
                                setDraft(value ?? "");
                                setEditorDirty(true);
                            }}
                        />
                    ))}
                </div>

                <footer className="native-config-modal__footer">
                    <div>
                        <span className={`badge badge--${dirty ? "warning" : queued?.status === "pending" ? "info" : "success"}`}>{t(dirty ? "server_detail.native_config.unsaved" : queued?.status === "pending" ? "server_detail.native_config.pending" : "server_detail.native_config.up_to_date")}</span>
                        <small>{t(isRunning ? "server_detail.native_config.running_hint" : "server_detail.native_config.stopped_hint")}</small>
                    </div>
                    <div>
                        {dirty && <Button type="button" size="sm" variant="ghost" icon={<RefreshCw size={15} />} onClick={() => void loadDocument(false)}>{t("server_detail.native_config.discard")}</Button>}
                        {queued?.status === "pending" && canWrite && <Button type="button" size="sm" variant="secondary" disabled={saving} onClick={() => void cancelQueue()}>{t("server_detail.native_config.cancel_queue")}</Button>}
                        {canWrite && <Button type="button" size="sm" icon={<Save size={15} />} disabled={!dirty || sourceChanged || Boolean(jsonError)} isLoading={saving} onClick={() => void queue()}>{t("server_detail.native_config.queue")}</Button>}
                    </div>
                </footer>
            </div>
        </div>
    );
}
