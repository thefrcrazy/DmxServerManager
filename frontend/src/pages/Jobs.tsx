import { Ban, ExternalLink, ListChecks, LoaderCircle, RefreshCw, Terminal } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { BedrockArchiveUploadNotice, HytaleDeviceAuthorizationNotice } from "@/components/features/server";
import { Button } from "@/components/ui";
import { useAuth } from "@/contexts/AuthContext";
import { useDialog } from "@/contexts/DialogContext";
import { useLanguage } from "@/contexts/LanguageContext";
import { usePageTitle } from "@/contexts/PageTitleContext";
import { useToast } from "@/contexts/ToastContext";
import { useGlobalEvents, usePermission } from "@/hooks";
import { JobSchema } from "@/schemas/api";
import type { Instance, Job } from "@/schemas/api";
import { BedrockArchiveAuthorizationSchema, HytaleDeviceAuthorizationSchema } from "@/schemas/operations";
import { apiService } from "@/services";

const JOB_EVENTS = ["job.updated", "job.waiting_for_user"] as const;
const ACTIVE_STATES = new Set<Job["state"]>(["queued", "running", "waiting_for_user"]);
const STATE_VALUES: Array<Job["state"]> = [
    "queued",
    "running",
    "waiting_for_user",
    "succeeded",
    "failed",
    "cancelled",
    "interrupted",
];

function stateBadge(state: Job["state"]): string {
    if (state === "succeeded") return "success";
    if (state === "failed" || state === "interrupted") return "danger";
    if (state === "waiting_for_user") return "warning";
    if (state === "running") return "info";
    return "neutral";
}

function translatedOrValue(t: (key: string) => string, key: string, fallback: string): string {
    const translated = t(key);
    return translated === key ? fallback : translated;
}

export default function Jobs() {
    const { t, language } = useLanguage();
    const { user } = useAuth();
    const { setPageTitle } = usePageTitle();
    const { hasPermission } = usePermission();
    const { confirm } = useDialog();
    const toast = useToast();
    const [searchParams, setSearchParams] = useSearchParams();
    const [jobs, setJobs] = useState<Job[]>([]);
    const [instances, setInstances] = useState<Instance[]>([]);
    const [loading, setLoading] = useState(true);
    const [loadError, setLoadError] = useState<string | null>(null);
    const [cancellingId, setCancellingId] = useState<string | null>(null);
    const canRead = hasPermission("job.read");
    const canCancel = hasPermission("server.update_game");
    const canUploadBedrockArchive = user?.role === "owner" && hasPermission("server.files.write");
    const stateFilter = searchParams.get("state") ?? "all";
    const instanceFilter = searchParams.get("instance") ?? "all";
    const focusedJobId = searchParams.get("focus");

    useEffect(() => {
        setPageTitle(t("jobs.title"), t("jobs.subtitle"));
    }, [setPageTitle, t]);

    const reload = useCallback(async () => {
        if (!canRead) return;
        const [jobsResponse, instancesResponse] = await Promise.all([
            apiService.jobs.list(),
            apiService.servers.getServers(),
        ]);
        if (!jobsResponse.success) {
            setLoadError(jobsResponse.error.message);
            setLoading(false);
            return;
        }
        setJobs(jobsResponse.data);
        if (instancesResponse.success) setInstances(instancesResponse.data);
        setLoadError(null);
        setLoading(false);
    }, [canRead]);

    useEffect(() => {
        setLoading(true);
        void reload();
    }, [reload]);

    const onEvent = useCallback((event: { type: string; payload: unknown }) => {
        if (event.type === "job.updated") {
            const parsed = JobSchema.safeParse(event.payload);
            if (parsed.success) {
                setJobs((current) => [parsed.data, ...current.filter((job) => job.id !== parsed.data.id)]);
                return;
            }
        }
        void reload();
    }, [reload]);
    const { isConnected } = useGlobalEvents({
        enabled: canRead,
        eventTypes: JOB_EVENTS,
        onEvent,
        onResynchronize: reload,
    });

    useEffect(() => {
        if (!focusedJobId || loading) return;
        document.getElementById(`job-${focusedJobId}`)?.scrollIntoView({ block: "center" });
    }, [focusedJobId, loading]);

    const instanceNames = useMemo(() => new Map(instances.map((instance) => [instance.id, instance.name])), [instances]);
    const filteredJobs = useMemo(() => jobs.filter((job) => {
        if (stateFilter !== "all" && job.state !== stateFilter) return false;
        return instanceFilter === "all" || job.instance_id === instanceFilter;
    }), [instanceFilter, jobs, stateFilter]);

    const updateFilter = (key: "state" | "instance", value: string) => {
        const next = new URLSearchParams(searchParams);
        if (value === "all") next.delete(key);
        else next.set(key, value);
        next.delete("focus");
        setSearchParams(next, { replace: true });
    };

    const cancelJob = async (job: Job) => {
        const accepted = await confirm(t("jobs.cancel_confirm"), {
            title: t("jobs.cancel"),
            confirmLabel: t("jobs.cancel"),
            isDestructive: true,
        });
        if (!accepted) return;
        setCancellingId(job.id);
        const response = await apiService.jobs.cancel(job.id);
        setCancellingId(null);
        if (!response.success) return toast.error(response.error.message);
        setJobs((current) => current.map((item) => item.id === response.data.id ? response.data : item));
        toast.success(t("jobs.cancelled"));
    };

    if (!canRead) return <div className="operations-access-denied" role="alert">{t("jobs.access_denied")}</div>;

    return (
        <section className="jobs-page" aria-labelledby="jobs-heading">
            <div className="operations-toolbar card">
                <div>
                    <h2 id="jobs-heading">{t("jobs.list_title")}</h2>
                    <p>{jobs.length} {t("jobs.title").toLocaleLowerCase(language === "fr" ? "fr-FR" : "en-US")}</p>
                </div>
                <div className="operations-toolbar__actions">
                    <span className="operations-status" role="status">
                        <span className={`connection-dot ${isConnected ? "connection-dot--online" : ""}`} aria-hidden="true" />
                        {isConnected ? t("realtime.connected") : t("realtime.reconnecting")}
                    </span>
                    <Button type="button" variant="secondary" size="sm" icon={<RefreshCw size={16} aria-hidden="true" />} onClick={() => void reload()}>
                        {t("jobs.refresh")}
                    </Button>
                </div>
            </div>

            <div className="jobs-filters card" aria-label={t("jobs.filter")}>
                <div className="form-group">
                    <label htmlFor="jobs-state-filter">{t("jobs.filter")}</label>
                    <select id="jobs-state-filter" className="input" value={stateFilter} onChange={(event) => updateFilter("state", event.target.value)}>
                        <option value="all">{t("jobs.all_states")}</option>
                        {STATE_VALUES.map((state) => <option key={state} value={state}>{t(`jobs.states.${state}`)}</option>)}
                    </select>
                </div>
                <div className="form-group">
                    <label htmlFor="jobs-instance-filter">{t("jobs.instance_filter")}</label>
                    <select id="jobs-instance-filter" className="input" value={instanceFilter} onChange={(event) => updateFilter("instance", event.target.value)}>
                        <option value="all">{t("jobs.all_instances")}</option>
                        {instances.map((instance) => <option key={instance.id} value={instance.id}>{instance.name}</option>)}
                    </select>
                </div>
            </div>

            <div className="job-list" aria-live="polite">
                {loading && <div className="operations-loading card"><LoaderCircle className="spinner" aria-hidden="true" />{t("common.loading")}</div>}
                {loadError && <div className="operations-error card" role="alert">{loadError}<Button size="sm" variant="secondary" onClick={() => void reload()}>{t("jobs.refresh")}</Button></div>}
                {!loading && !loadError && filteredJobs.length === 0 && (
                    <div className="operations-empty card"><ListChecks aria-hidden="true" /><p>{t("jobs.empty")}</p></div>
                )}
                {filteredJobs.map((job) => {
                    const instanceName = job.instance_id ? instanceNames.get(job.instance_id) : undefined;
                    const cancellable = canCancel && job.kind === "install" && ACTIVE_STATES.has(job.state);
                    const interactionEnvelope = job.interaction ? { job_id: job.id, interaction: job.interaction } : null;
                    const hytaleAuthorization = HytaleDeviceAuthorizationSchema.safeParse(interactionEnvelope);
                    const bedrockAuthorization = BedrockArchiveAuthorizationSchema.safeParse(interactionEnvelope);
                    return (
                        <article id={`job-${job.id}`} key={job.id} className={`job-card card ${focusedJobId === job.id ? "job-card--focused" : ""}`}>
                            <header className="job-card__header">
                                <div>
                                    <h3>{translatedOrValue(t, `jobs.kinds.${job.kind}`, job.kind)}</h3>
                                    <code>{job.id}</code>
                                </div>
                                <span className={`badge badge--${stateBadge(job.state)}`}>{t(`jobs.states.${job.state}`)}</span>
                            </header>
                            <div className="job-card__metadata">
                                <div><span>{t("jobs.instance_filter")}</span>{job.instance_id && instanceName
                                    ? <Link to={`/servers/${job.instance_id}`}>{instanceName}<ExternalLink size={13} aria-hidden="true" /></Link>
                                    : <strong>{job.instance_id ? t("jobs.unknown_instance") : t("common.system")}</strong>}</div>
                                <div><span>{t("jobs.created")}</span><time dateTime={job.created_at}>{new Date(job.created_at).toLocaleString(language === "fr" ? "fr-FR" : "en-US")}</time></div>
                                <div><span>{t("jobs.finished")}</span><time dateTime={job.finished_at ?? undefined}>{job.finished_at ? new Date(job.finished_at).toLocaleString(language === "fr" ? "fr-FR" : "en-US") : "—"}</time></div>
                            </div>
                            <div className="job-card__progress">
                                <div><span>{t("jobs.progress")}</span><strong>{job.progress} %</strong></div>
                                <progress max={100} value={job.progress} aria-label={`${t("jobs.progress")} ${job.progress} %`} />
                            </div>
                            {hytaleAuthorization.success && (
                                <div className="job-card__interaction">
                                    <HytaleDeviceAuthorizationNotice authorization={hytaleAuthorization.data} />
                                </div>
                            )}
                            {bedrockAuthorization.success && (
                                <div className="job-card__interaction">
                                    <BedrockArchiveUploadNotice
                                        authorization={bedrockAuthorization.data}
                                        canUpload={canUploadBedrockArchive}
                                        onAccepted={(updated) => setJobs((current) => current.map((item) => item.id === updated.id ? updated : item))}
                                    />
                                </div>
                            )}
                            {(job.error_code || job.error_message) && (
                                <div className="job-card__error" role="alert">
                                    <strong>{job.error_code ?? t("jobs.error")}</strong>
                                    {job.error_message && <span>{job.error_message}</span>}
                                </div>
                            )}
                            <footer className="job-card__actions">
                                {job.instance_id && instanceName && job.kind === "install" && <Button as="link" to={`/servers/${job.instance_id}?tab=console&source=install&job=${job.id}`} variant="secondary" size="sm" icon={<Terminal size={15} aria-hidden="true" />}>{t("jobs.view_install_terminal")}</Button>}
                                {job.instance_id && instanceName && <Button as="link" to={`/servers/${job.instance_id}`} variant="ghost" size="sm" icon={<ExternalLink size={15} aria-hidden="true" />}>{t("jobs.view_instance")}</Button>}
                                {cancellable && <Button type="button" variant="danger" size="sm" isLoading={cancellingId === job.id} icon={<Ban size={15} aria-hidden="true" />} onClick={() => void cancelJob(job)}>{t("jobs.cancel")}</Button>}
                            </footer>
                        </article>
                    );
                })}
            </div>
        </section>
    );
}
