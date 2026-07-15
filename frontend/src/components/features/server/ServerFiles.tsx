import { Download, File, FilePenLine, Folder, FolderPlus, LoaderCircle, Save, Trash2, Upload } from "lucide-react";
import { ChangeEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui";
import { useDialog } from "@/contexts/DialogContext";
import { useLanguage } from "@/contexts/LanguageContext";
import { useToast } from "@/contexts/ToastContext";
import { ManagedFileEntry } from "@/schemas/operations";
import { apiService } from "@/services";
import { safeDownloadName } from "@/services/api/query";
import { formatBytes } from "@/utils/formatters";

const MAX_UPLOAD_BYTES = 1_024 * 1_024;
const MAX_TEXT_BYTES = 512 * 1_024;

interface ServerFilesProps {
    instanceId: string;
    canWrite: boolean;
    isStopped: boolean;
    refreshSignal?: number;
}

interface TextEditorState {
    path: string;
    content: string;
    savedContent: string;
}

function joinPath(parent: string, name: string): string {
    return parent ? `${parent}/${name}` : name;
}

function validSegment(value: string): boolean {
    return value.length > 0
        && value.length <= 255
        && value !== "."
        && value !== ".."
        && !value.includes("/")
        && !value.includes("\\")
        && !value.includes(":")
        && !/[\u0000-\u001f\u007f]/.test(value);
}

export default function ServerFiles({ instanceId, canWrite, isStopped, refreshSignal = 0 }: ServerFilesProps) {
    const { t, language } = useLanguage();
    const toast = useToast();
    const { confirm, prompt } = useDialog();
    const uploadRef = useRef<HTMLInputElement>(null);
    const [currentPath, setCurrentPath] = useState("");
    const [entries, setEntries] = useState<ManagedFileEntry[]>([]);
    const [editor, setEditor] = useState<TextEditorState | null>(null);
    const [loading, setLoading] = useState(true);
    const [mutating, setMutating] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const canMutate = canWrite && isStopped;

    const load = useCallback(async () => {
        setLoading(true);
        const response = await apiService.files.list(instanceId, currentPath);
        setLoading(false);
        if (!response.success) {
            setError(response.error.message);
            return;
        }
        setEntries(response.data);
        setError(null);
    }, [currentPath, instanceId]);

    useEffect(() => { void load(); }, [load, refreshSignal]);

    const breadcrumbs = useMemo(() => {
        const parts = currentPath.split("/").filter(Boolean);
        return parts.map((label, index) => ({ label, path: parts.slice(0, index + 1).join("/") }));
    }, [currentPath]);

    const openText = async (entry: ManagedFileEntry) => {
        if (entry.size_bytes > MAX_TEXT_BYTES) return toast.error(t("server_detail.files.text_too_large"));
        const response = await apiService.files.readText(instanceId, entry.path);
        if (!response.success) return toast.error(response.error.message);
        setEditor({ path: entry.path, content: response.data.content, savedContent: response.data.content });
    };

    const saveText = async () => {
        if (!editor || !canMutate) return;
        if (new TextEncoder().encode(editor.content).byteLength > MAX_TEXT_BYTES) {
            return toast.error(t("server_detail.files.text_too_large"));
        }
        setMutating(true);
        const response = await apiService.files.writeText(instanceId, editor.path, editor.content);
        setMutating(false);
        if (!response.success) return toast.error(response.error.message);
        setEditor((current) => current ? { ...current, savedContent: current.content } : null);
        toast.success(t("server_detail.files.saved"));
        await load();
    };

    const createDirectory = async () => {
        const name = (await prompt(t("server_detail.files.folder_name"), { confirmLabel: t("common.create") }))?.trim();
        if (!name) return;
        if (!validSegment(name)) return toast.error(t("server_detail.files.invalid_name"));
        setMutating(true);
        const response = await apiService.files.createDirectory(instanceId, joinPath(currentPath, name));
        setMutating(false);
        if (!response.success) return toast.error(response.error.message);
        toast.success(t("server_detail.files.folder_created"));
        await load();
    };

    const upload = async (event: ChangeEvent<HTMLInputElement>) => {
        const selected = event.target.files?.[0];
        event.target.value = "";
        if (!selected) return;
        if (!validSegment(selected.name)) return toast.error(t("server_detail.files.invalid_name"));
        if (selected.size > MAX_UPLOAD_BYTES) return toast.error(t("server_detail.files.upload_too_large"));
        setMutating(true);
        const response = await apiService.files.upload(instanceId, joinPath(currentPath, selected.name), selected);
        setMutating(false);
        if (!response.success) return toast.error(response.error.message);
        toast.success(t("server_detail.files.uploaded"));
        await load();
    };

    const remove = async (entry: ManagedFileEntry) => {
        const accepted = await confirm(`${t("server_detail.files.delete_confirm")} « ${entry.name} » ?`, { isDestructive: true });
        if (!accepted) return;
        setMutating(true);
        const response = await apiService.files.remove(instanceId, entry.path);
        setMutating(false);
        if (!response.success) return toast.error(response.error.message);
        if (editor?.path === entry.path) setEditor(null);
        toast.success(t("server_detail.files.deleted"));
        await load();
    };

    return (
        <section className="server-files card" aria-labelledby="files-heading">
            <div className="server-files__toolbar">
                <nav className="file-breadcrumbs" aria-label={t("server_detail.files.breadcrumb_label")}>
                    <button type="button" onClick={() => setCurrentPath("")}>{t("server_detail.files.root")}</button>
                    {breadcrumbs.map((part) => <span key={part.path}>/<button type="button" onClick={() => setCurrentPath(part.path)}>{part.label}</button></span>)}
                </nav>
                <div className="server-files__actions">
                    <Button size="sm" variant="secondary" icon={<FolderPlus size={16} aria-hidden="true" />} disabled={!canMutate || mutating} onClick={() => void createDirectory()}>
                        {t("common.new_folder")}
                    </Button>
                    <Button size="sm" icon={<Upload size={16} aria-hidden="true" />} disabled={!canMutate || mutating} onClick={() => uploadRef.current?.click()}>
                        {t("common.upload")}
                    </Button>
                    <input ref={uploadRef} className="sr-only" type="file" onChange={(event) => void upload(event)} aria-label={t("server_detail.files.upload_label")} />
                </div>
            </div>
            <h2 id="files-heading" className="sr-only">{t("server_detail.tabs.files")}</h2>
            {canWrite && !isStopped && <p className="operations-warning" role="status">{t("server_detail.files.stop_to_edit")}</p>}
            <div className={`server-files__layout ${editor ? "server-files__layout--editor" : ""}`}>
                <div className="file-browser">
                    {loading && <div className="operations-loading"><LoaderCircle className="spinner" aria-hidden="true" />{t("common.loading")}</div>}
                    {error && <div className="operations-error" role="alert">{error}<Button size="sm" variant="secondary" onClick={() => void load()}>{t("administration.retry")}</Button></div>}
                    {!loading && !error && entries.length === 0 && <div className="operations-empty"><Folder aria-hidden="true" /><p>{t("server_detail.files.empty_folder")}</p></div>}
                    {!loading && !error && entries.length > 0 && (
                        <div className="table-scroll">
                            <table className="file-table">
                                <thead><tr><th>{t("server_detail.files.name")}</th><th>{t("server_detail.files.size")}</th><th>{t("server_detail.files.modified")}</th><th><span className="sr-only">{t("common.actions")}</span></th></tr></thead>
                                <tbody>{entries.map((entry) => (
                                    <tr key={entry.path}>
                                        <td>
                                            <button type="button" className="file-name-button" onClick={() => entry.kind === "directory" ? setCurrentPath(entry.path) : void openText(entry)}>
                                                {entry.kind === "directory" ? <Folder size={17} aria-hidden="true" /> : <File size={17} aria-hidden="true" />}
                                                <span>{entry.name}</span>
                                            </button>
                                        </td>
                                        <td>{entry.kind === "file" ? formatBytes(entry.size_bytes) : "—"}</td>
                                        <td>{entry.modified_at ? new Date(entry.modified_at).toLocaleString(language === "fr" ? "fr-FR" : "en-US") : "—"}</td>
                                        <td><div className="file-row-actions">
                                            {entry.kind === "file" && <a className="icon-action" href={apiService.files.downloadUrl(instanceId, entry.path)} download={safeDownloadName(entry.name, "download")} aria-label={`${t("common.download")} ${entry.name}`}><Download size={15} aria-hidden="true" /></a>}
                                            {entry.kind === "file" && entry.size_bytes <= MAX_TEXT_BYTES && <button type="button" className="icon-action" onClick={() => void openText(entry)} aria-label={`${t("common.edit")} ${entry.name}`}><FilePenLine size={15} aria-hidden="true" /></button>}
                                            {canMutate && <button type="button" className="icon-action icon-action--danger" onClick={() => void remove(entry)} aria-label={`${t("common.delete")} ${entry.name}`}><Trash2 size={15} aria-hidden="true" /></button>}
                                        </div></td>
                                    </tr>
                                ))}</tbody>
                            </table>
                        </div>
                    )}
                </div>
                {editor && (
                    <div className="text-editor">
                        <div className="text-editor__header">
                            <strong>{editor.path}</strong>
                            <button type="button" className="icon-action" onClick={() => setEditor(null)} aria-label={t("server_detail.files.close_editor")}>×</button>
                        </div>
                        <label htmlFor="managed-text-editor" className="sr-only">{`${t("common.edit")} ${editor.path}`}</label>
                        <textarea id="managed-text-editor" spellCheck={false} value={editor.content} onChange={(event) => setEditor((current) => current ? { ...current, content: event.target.value } : null)} readOnly={!canMutate} />
                        <div className="text-editor__footer">
                            <span>{formatBytes(new TextEncoder().encode(editor.content).byteLength)} / {formatBytes(MAX_TEXT_BYTES)}</span>
                            <Button size="sm" icon={<Save size={16} aria-hidden="true" />} disabled={!canMutate || editor.content === editor.savedContent} isLoading={mutating} onClick={() => void saveText()}>{t("common.save")}</Button>
                        </div>
                    </div>
                )}
            </div>
        </section>
    );
}
