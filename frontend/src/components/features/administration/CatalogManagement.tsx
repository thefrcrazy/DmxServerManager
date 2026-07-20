import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
    Boxes,
    Check,
    FileArchive,
    LoaderCircle,
    Palette,
    RefreshCw,
    RotateCcw,
    ShieldCheck,
    Trash2,
    Upload,
} from "lucide-react";
import { Link } from "react-router-dom";
import { Button } from "@/components/ui";
import { useDialog } from "@/contexts/DialogContext";
import { useLanguage } from "@/contexts/LanguageContext";
import { useTheme } from "@/contexts/ThemeContext";
import { useToast } from "@/contexts/ToastContext";
import type { Job } from "@/schemas/api";
import type { CatalogPackage, ThemeSelection } from "@/schemas/catalog";
import { apiService } from "@/services";

const MAX_PACKAGE_BYTES = 16 * 1024 * 1024;
const ACTIVE_JOB_STATES = new Set<Job["state"]>(["queued", "running", "waiting_for_user"]);

function packageKey(item: Pick<CatalogPackage, "kind" | "id">): string {
    return `${item.kind}:${item.id}`;
}

function assetUrl(item: CatalogPackage, role: "icon" | "logo" | "preview"): string | null {
    if (!item.files.some((file) => file.role === role)) return null;
    return `/api/v1/catalog/${item.kind}/${item.id}/revisions/${item.revision}/assets/${role}`;
}

export async function sha256Package(file: File): Promise<string> {
    const digest = await crypto.subtle.digest("SHA-256", await file.arrayBuffer());
    return [...new Uint8Array(digest)]
        .map((byte) => byte.toString(16).padStart(2, "0"))
        .join("");
}

export default function CatalogManagement() {
    const { t, language } = useLanguage();
    const { confirm } = useDialog();
    const toast = useToast();
    const { activeTheme, refreshTheme } = useTheme();
    const fileInput = useRef<HTMLInputElement>(null);
    const [packages, setPackages] = useState<CatalogPackage[]>([]);
    const [selectedKey, setSelectedKey] = useState<string | null>(null);
    const [revisions, setRevisions] = useState<CatalogPackage[]>([]);
    const [loading, setLoading] = useState(true);
    const [loadingRevisions, setLoadingRevisions] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [file, setFile] = useState<File | null>(null);
    const [hashing, setHashing] = useState(false);
    const [job, setJob] = useState<Job | null>(null);
    const [deleting, setDeleting] = useState<string | null>(null);
    const [selectingTheme, setSelectingTheme] = useState<string | null>(null);

    const translatedError = useCallback((value: string, fallback: string) => {
        const translated = t(value);
        return translated === value ? fallback : translated;
    }, [t]);

    const load = useCallback(async () => {
        setLoading(true);
        const response = await apiService.catalog.list();
        setLoading(false);
        if (!response.success) {
            setError(translatedError(response.error.message, t("administration.catalog.load_error")));
            return;
        }
        setPackages(response.data);
        setError(null);
    }, [t, translatedError]);

    const loadRevisions = useCallback(async (item: CatalogPackage) => {
        setSelectedKey(packageKey(item));
        setLoadingRevisions(true);
        const response = await apiService.catalog.revisions(item.kind, item.id);
        setLoadingRevisions(false);
        if (!response.success) {
            toast.error(translatedError(response.error.message, t("administration.catalog.load_error")));
            return;
        }
        setRevisions(response.data);
    }, [t, toast, translatedError]);

    useEffect(() => {
        void load();
    }, [load]);

    useEffect(() => {
        if (!job || !ACTIVE_JOB_STATES.has(job.state)) return;
        let stopped = false;
        const timer = window.setInterval(() => {
            void apiService.jobs.get(job.id).then(async (response) => {
                if (stopped || !response.success) return;
                setJob(response.data);
                if (ACTIVE_JOB_STATES.has(response.data.state)) return;
                window.clearInterval(timer);
                if (response.data.state === "succeeded") {
                    setFile(null);
                    if (fileInput.current) fileInput.current.value = "";
                    toast.success(t("administration.catalog.import_succeeded"));
                    await load();
                } else {
                    toast.error(translatedError(
                        response.data.error_message ?? response.data.error_code ?? "",
                        t("administration.catalog.import_failed"),
                    ));
                }
            });
        }, 750);
        return () => {
            stopped = true;
            window.clearInterval(timer);
        };
    }, [job, load, t, toast, translatedError]);

    const selected = useMemo(
        () => packages.find((item) => packageKey(item) === selectedKey) ?? null,
        [packages, selectedKey],
    );

    const importPackage = async () => {
        if (!file) return;
        if (file.size === 0 || file.size > MAX_PACKAGE_BYTES || !file.name.toLowerCase().endsWith(".dmxpack")) {
            toast.error(t("administration.catalog.invalid_file"));
            return;
        }
        setHashing(true);
        let checksum: string;
        try {
            checksum = await sha256Package(file);
        } catch {
            setHashing(false);
            toast.error(t("administration.catalog.hash_failed"));
            return;
        }
        const response = await apiService.catalog.importPackage(
            file,
            checksum,
            `catalog-ui-${crypto.randomUUID()}`,
        );
        setHashing(false);
        if (!response.success) {
            toast.error(translatedError(response.error.message, t("administration.catalog.import_failed")));
            return;
        }
        setJob(response.data);
        toast.info(t("administration.catalog.import_queued"));
    };

    const removeRevision = async (item: CatalogPackage) => {
        const accepted = await confirm(t("administration.catalog.delete_confirm"), {
            title: t("administration.catalog.delete_title"),
            confirmLabel: t("common.delete"),
            isDestructive: true,
        });
        if (!accepted) return;
        const key = `${packageKey(item)}:${item.revision}`;
        setDeleting(key);
        const response = await apiService.catalog.deleteRevision(item.kind, item.id, item.revision);
        setDeleting(null);
        if (!response.success) {
            toast.error(translatedError(response.error.message, t("administration.catalog.delete_failed")));
            return;
        }
        toast.success(t("administration.catalog.deleted"));
        const remaining = revisions.filter((revision) => revision.revision !== item.revision);
        setRevisions(remaining);
        if (remaining.length === 0) setSelectedKey(null);
        await load();
    };

    const selectTheme = async (selection: ThemeSelection) => {
        const key = selection.kind === "default"
            ? "default"
            : `${selection.package_id}:${selection.revision}`;
        setSelectingTheme(key);
        const response = await apiService.catalog.selectTheme(selection, activeTheme.version);
        setSelectingTheme(null);
        if (!response.success) {
            toast.error(translatedError(response.error.message, t("administration.catalog.theme_failed")));
            await refreshTheme();
            return;
        }
        await refreshTheme();
        toast.success(t("administration.catalog.theme_applied"));
    };

    const formatDate = (value: string) => new Intl.DateTimeFormat(
        language === "fr" ? "fr-FR" : "en-US",
        { dateStyle: "medium", timeStyle: "short" },
    ).format(new Date(value));

    return (
        <section className="administration-panel catalog-management" aria-labelledby="catalog-heading">
            <div className="administration-panel__heading">
                <div>
                    <h2 id="catalog-heading">{t("administration.catalog.title")}</h2>
                    <p>{t("administration.catalog.description")}</p>
                </div>
                <Button type="button" variant="secondary" icon={<RefreshCw size={17} />} onClick={() => void load()}>
                    {t("common.refresh")}
                </Button>
            </div>

            {error && <div className="administration-alert administration-alert--error" role="alert">{error}</div>}

            <div className="catalog-upload card">
                <div className="catalog-upload__icon"><FileArchive size={24} aria-hidden="true" /></div>
                <div className="catalog-upload__content">
                    <label htmlFor="catalog-package-file">{t("administration.catalog.import_label")}</label>
                    <p>{t("administration.catalog.import_hint")}</p>
                    <input
                        ref={fileInput}
                        id="catalog-package-file"
                        type="file"
                        accept=".dmxpack,application/vnd.dmxpack+zip,application/zip"
                        onChange={(event) => setFile(event.target.files?.[0] ?? null)}
                    />
                </div>
                <Button
                    type="button"
                    icon={<Upload size={17} />}
                    disabled={!file || Boolean(job && ACTIVE_JOB_STATES.has(job.state))}
                    isLoading={hashing}
                    onClick={() => void importPackage()}
                >
                    {hashing ? t("administration.catalog.hashing") : t("administration.catalog.import")}
                </Button>
            </div>

            {job && (
                <div className="catalog-job card" aria-live="polite">
                    {ACTIVE_JOB_STATES.has(job.state) ? <LoaderCircle className="spinner" aria-hidden="true" /> : <Check size={20} aria-hidden="true" />}
                    <div>
                        <strong>{t(`jobs.states.${job.state}`)}</strong>
                        <progress max={100} value={job.progress} aria-label={`${t("jobs.progress")} ${job.progress} %`} />
                    </div>
                    <span>{job.progress} %</span>
                    <Link to={`/activity?tab=operations&focus=${encodeURIComponent(job.id)}`}>{t("administration.catalog.view_job")}</Link>
                </div>
            )}

            <div className="catalog-theme-default card">
                <div><Palette size={22} aria-hidden="true" /><div><h3>{t("administration.catalog.default_theme")}</h3><p>{t("administration.catalog.default_theme_hint")}</p></div></div>
                {activeTheme.selection.kind === "default" ? (
                    <span className="badge badge--success"><Check size={13} />{t("administration.catalog.active")}</span>
                ) : (
                    <Button type="button" variant="secondary" size="sm" icon={<RotateCcw size={15} />} isLoading={selectingTheme === "default"} onClick={() => void selectTheme({ kind: "default" })}>
                        {t("administration.catalog.restore_default")}
                    </Button>
                )}
            </div>

            {loading ? (
                <div className="administration-loading" role="status"><span className="spinner spinner--sm" />{t("common.loading")}</div>
            ) : packages.length === 0 ? (
                <div className="card administration-empty"><Boxes size={30} aria-hidden="true" /><p>{t("administration.catalog.empty")}</p></div>
            ) : (
                <div className="catalog-grid">
                    <div className="catalog-list" role="group" aria-label={t("administration.catalog.packages")}>
                        {packages.map((item) => {
                            const image = assetUrl(item, item.kind === "theme" ? "preview" : "icon");
                            return (
                                <button
                                    type="button"
                                    key={packageKey(item)}
                                    className={`catalog-card card ${selectedKey === packageKey(item) ? "catalog-card--active" : ""}`}
                                    aria-pressed={selectedKey === packageKey(item)}
                                    onClick={() => void loadRevisions(item)}
                                >
                                    <span className="catalog-card__visual">
                                        {image ? <img src={image} alt="" /> : item.kind === "theme" ? <Palette aria-hidden="true" /> : <Boxes aria-hidden="true" />}
                                    </span>
                                    <span className="catalog-card__body"><strong>{item.name}</strong><small>{item.description}</small></span>
                                    <span className="badge badge--neutral">{t("administration.catalog.version")} {item.revision}</span>
                                </button>
                            );
                        })}
                    </div>

                    <div className="catalog-revisions card">
                        {!selected && <div className="administration-empty"><ShieldCheck size={28} aria-hidden="true" /><p>{t("administration.catalog.select_package")}</p></div>}
                        {selected && <>
                            <header><div><h3>{selected.name}</h3><p>{selected.id}</p></div><span className="badge badge--info">{t(`administration.catalog.kinds.${selected.kind}`)}</span></header>
                            {loadingRevisions ? <div className="administration-loading" role="status"><span className="spinner spinner--sm" />{t("common.loading")}</div> : (
                                <ul className="catalog-revision-list">
                                    {revisions.map((item) => {
                                        const active = activeTheme.selection.kind === "catalog"
                                            && activeTheme.selection.package_id === item.id
                                            && activeTheme.selection.revision === item.revision;
                                        const deleteKey = `${packageKey(item)}:${item.revision}`;
                                        return <li key={item.revision}>
                                            <div><strong>{t("administration.catalog.version")} {item.revision}</strong><time dateTime={item.created_at}>{formatDate(item.created_at)}</time><code title={item.archive_sha256}>{item.archive_sha256.slice(0, 12)}…</code></div>
                                            <div className="catalog-revision-list__actions">
                                                {active && <span className="badge badge--success"><Check size={13} />{t("administration.catalog.active")}</span>}
                                                {item.kind === "theme" && !active && <Button type="button" size="sm" variant="secondary" isLoading={selectingTheme === `${item.id}:${item.revision}`} onClick={() => void selectTheme({ kind: "catalog", package_id: item.id, revision: item.revision })}>{t("administration.catalog.apply_theme")}</Button>}
                                                <Button type="button" size="sm" variant="danger" icon={<Trash2 size={15} />} disabled={active} isLoading={deleting === deleteKey} onClick={() => void removeRevision(item)}>{t("common.delete")}</Button>
                                            </div>
                                        </li>;
                                    })}
                                </ul>
                            )}
                        </>}
                    </div>
                </div>
            )}
        </section>
    );
}
