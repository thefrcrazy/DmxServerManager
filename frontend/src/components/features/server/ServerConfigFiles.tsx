import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import { Clock3, FileCode2, LoaderCircle, Pencil, RefreshCw, ShieldAlert } from "lucide-react";
import { Button } from "@/components/ui";
import { useLanguage } from "@/contexts/LanguageContext";
import type { ConfigFileCategory, ConfigFileSummary } from "@/schemas/operations";
import { apiService } from "@/services";
import { formatBytes, formatDate } from "@/utils/formatters";
import NativeAccessListForm from "./NativeAccessListForm";

const NativeConfigEditorModal = lazy(() => import("./NativeConfigEditorModal"));

interface ServerConfigFilesProps {
    instanceId: string;
    category: ConfigFileCategory;
    canReadRaw: boolean;
    canWriteRaw: boolean;
    isRunning: boolean;
    refreshSignal?: number;
}

const LIVE_REFRESH_MS = 5_000;

function roleKey(file: ConfigFileSummary): string {
    const filename = file.path.split("/").at(-1)?.toLowerCase() ?? "";
    if (/admin|operator|permission|role/.test(filename)) return "administration";
    if (/ban|blacklist/.test(filename)) return "bans";
    if (/white|allowlist/.test(filename)) return "allowlist";
    if (/world|universe/.test(filename)) return "world";
    if (/server|game|config|propert/.test(filename)) return "server";
    return file.category === "access" ? "access" : "advanced";
}

export default function ServerConfigFiles({
    instanceId,
    category,
    canReadRaw,
    canWriteRaw,
    isRunning,
    refreshSignal = 0,
}: ServerConfigFilesProps) {
    const { t, language } = useLanguage();
    const [files, setFiles] = useState<ConfigFileSummary[]>([]);
    const [editorFile, setEditorFile] = useState<ConfigFileSummary | null>(null);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);

    const loadList = useCallback(async () => {
        if (!canReadRaw) {
            setFiles([]);
            setEditorFile(null);
            setError(null);
            setLoading(false);
            return;
        }
        const response = await apiService.config.list(instanceId);
        setLoading(false);
        if (!response.success) {
            setError(response.error.message);
            return;
        }
        const next = response.data.items.filter((file) => file.category === category);
        setFiles(next);
        setEditorFile((current) => current ? next.find((file) => file.path === current.path) ?? current : null);
        setError(null);
    }, [canReadRaw, category, instanceId]);

    useEffect(() => {
        setLoading(true);
        void loadList();
        const timer = window.setInterval(() => void loadList(), LIVE_REFRESH_MS);
        return () => window.clearInterval(timer);
    }, [loadList, refreshSignal]);

    const locale = language === "fr" ? "fr-FR" : "en-US";

    return (
        <section className={`native-config-section native-config-section--${category}`} aria-labelledby={`native-config-${category}`}>
            <header className="native-config-section__header">
                <div>
                    <h3 id={`native-config-${category}`}><FileCode2 size={18} aria-hidden="true" />{t(`server_detail.native_config.${category}_title`)}</h3>
                    <p>{t(`server_detail.native_config.${category}_description`)}</p>
                </div>
                <Button type="button" size="sm" variant="ghost" icon={<RefreshCw size={15} />} onClick={() => void loadList()}>{t("common.refresh")}</Button>
            </header>

            <div className="native-config-apply-hint" role="note">
                <Clock3 size={16} aria-hidden="true" />
                <span>{t(isRunning ? "server_detail.native_config.running_hint" : "server_detail.native_config.stopped_hint")}</span>
            </div>

            {!canReadRaw && <div className="native-config-details__permission" role="note"><ShieldAlert size={16} aria-hidden="true" />{t("server_detail.native_config.raw_read_required")}</div>}
            {canReadRaw && loading && <div className="operations-loading"><LoaderCircle className="spinner" aria-hidden="true" />{t("common.loading")}</div>}
            {canReadRaw && error && <div className="operations-error" role="alert">{error}</div>}
            {canReadRaw && !loading && !error && files.length === 0 && <div className="operations-empty"><FileCode2 aria-hidden="true" /><p>{t("server_detail.native_config.empty")}</p></div>}

            {!loading && files.length > 0 && <div className="native-config-details-list">
                {files.map((file) => {
                    const filename = file.path.split("/").at(-1) ?? file.path;
                    const queued = file.queued_change;
                    const advancedOnly = file.format === "yaml" || file.format === "lua";
                    return <details className="native-config-details" key={file.path}>
                        <summary>
                            <span className="native-config-details__identity"><FileCode2 size={17} aria-hidden="true" /><span><strong>{filename}</strong><small>{t(`server_detail.native_config.roles.${roleKey(file)}`)}</small></span></span>
                            <span className="native-config-details__summary-meta">
                                <span>{file.format.toUpperCase()}</span>
                                <span className={`badge badge--${queued ? queued.status === "pending" ? "warning" : "danger" : file.exists ? "success" : "muted"}`}>{t(queued ? `server_detail.native_config.status.${queued.status}` : file.exists ? "server_detail.native_config.live" : "server_detail.native_config.missing")}</span>
                            </span>
                        </summary>
                        <div className="native-config-details__body">
                            <dl>
                                <div><dt>{t("server_detail.native_config.path")}</dt><dd><code>{file.path}</code></dd></div>
                                <div><dt>{t("server_detail.native_config.role")}</dt><dd>{t(`server_detail.native_config.roles.${roleKey(file)}`)}</dd></div>
                                <div><dt>{t("server_detail.native_config.format")}</dt><dd>{file.format.toUpperCase()}{advancedOnly ? ` · ${t("server_detail.native_config.advanced_only")}` : ""}</dd></div>
                                <div><dt>{t("server_detail.native_config.size")}</dt><dd>{formatBytes(file.size_bytes)}</dd></div>
                                <div><dt>{t("server_detail.native_config.modified")}</dt><dd>{file.modified_at ? formatDate(file.modified_at, locale) : t("server_detail.native_config.never_modified")}</dd></div>
                            </dl>
                            {queued && <p className={`native-config-details__status native-config-details__status--${queued.status}`}>{t(`server_detail.native_config.status_detail.${queued.status}`)}</p>}
                            {category === "access" && file.format === "text" && canReadRaw && <NativeAccessListForm instanceId={instanceId} file={file} canWrite={canWriteRaw} onChanged={() => void loadList()} />}
                            <div className="native-config-details__actions">
                                {!canReadRaw ? <span className="native-config-details__permission"><ShieldAlert size={16} />{t("server_detail.native_config.raw_read_required")}</span> : <Button type="button" size="sm" variant="secondary" icon={<Pencil size={15} />} onClick={() => setEditorFile(file)}>{t(canWriteRaw ? "server_detail.native_config.modify" : "server_detail.native_config.inspect")}</Button>}
                            </div>
                        </div>
                    </details>;
                })}
            </div>}

            {editorFile && <Suspense fallback={<div className="native-config-modal-backdrop"><div className="native-config-modal native-config-modal--loading"><LoaderCircle className="spinner" /><span>{t("server_detail.native_config.loading_editor")}</span></div></div>}>
                <NativeConfigEditorModal
                    instanceId={instanceId}
                    file={editorFile}
                    canWrite={canWriteRaw}
                    isRunning={isRunning}
                    onClose={() => setEditorFile(null)}
                    onChanged={() => void loadList()}
                />
            </Suspense>}
        </section>
    );
}
