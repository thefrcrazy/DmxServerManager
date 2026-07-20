import {
    AlertTriangle,
    Ban,
    ChevronRight,
    CircleAlert,
    ExternalLink,
    History,
    ListChecks,
    RefreshCw,
    ShieldCheck,
    X,
} from "lucide-react";
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
import type { Instance, Job } from "@/schemas/api";
import {
    BedrockArchiveAuthorizationSchema,
    HytaleDeviceAuthorizationSchema,
    type ActivitySummary,
    type AuditEvent,
} from "@/schemas/operations";
import { apiService } from "@/services";

type ActivityTab = "attention" | "operations" | "journal";
const ACTIVE_STATES = new Set<Job["state"]>(["queued", "running", "waiting_for_user"]);
const EMPTY_SUMMARY: ActivitySummary = { active_jobs: 0, waiting_for_user: 0, failed_jobs_24h: 0, crashed_servers: 0, config_conflicts: 0 };

function badgeVariant(state: Job["state"]): string {
    if (state === "succeeded") return "success";
    if (state === "failed" || state === "interrupted") return "danger";
    if (state === "waiting_for_user") return "warning";
    if (state === "running") return "info";
    return "neutral";
}

export default function ActivityPage() {
    const { t, language } = useLanguage();
    const { user } = useAuth();
    const { setPageTitle } = usePageTitle();
    const { hasPermission } = usePermission();
    const { confirm } = useDialog();
    const toast = useToast();
    const [searchParams, setSearchParams] = useSearchParams();
    const requestedTab = searchParams.get("tab");
    const canReadAudit = (user?.role === "owner" || user?.role === "admin") && hasPermission("audit.read");
    const activeTab: ActivityTab = requestedTab === "journal" && canReadAudit
        ? "journal"
        : requestedTab === "operations" ? "operations" : "attention";
    const [summary, setSummary] = useState<ActivitySummary>(EMPTY_SUMMARY);
    const [jobs, setJobs] = useState<Job[]>([]);
    const [nextCursor, setNextCursor] = useState<string | null>(null);
    const [audit, setAudit] = useState<AuditEvent[]>([]);
    const [nextAuditId, setNextAuditId] = useState<number | null>(null);
    const [instances, setInstances] = useState<Instance[]>([]);
    const [selectedJob, setSelectedJob] = useState<Job | null>(null);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState("");
    const [cancelling, setCancelling] = useState(false);
    const stateFilter = searchParams.get("state") as Job["state"] | null;
    const instanceFilter = searchParams.get("instance") ?? undefined;

    useEffect(() => setPageTitle(t("activity.title"), t("activity.subtitle")), [setPageTitle, t]);

    const reload = useCallback(async () => {
        setLoading(true);
        const [summaryResponse, jobsResponse, serversResponse, auditResponse] = await Promise.all([
            apiService.activity.summary(),
            apiService.activity.jobs({ limit: 50, state: stateFilter ?? undefined, instance_id: instanceFilter }),
            apiService.servers.getServers(),
            canReadAudit && activeTab === "journal" ? apiService.activity.audit({ limit: 50 }) : Promise.resolve(null),
        ]);
        const failure = !summaryResponse.success ? summaryResponse.error
            : !jobsResponse.success ? jobsResponse.error
                : !serversResponse.success ? serversResponse.error
                    : auditResponse && !auditResponse.success ? auditResponse.error : null;
        if (failure) setError(failure.message);
        else setError("");
        if (summaryResponse.success) setSummary(summaryResponse.data);
        if (jobsResponse.success) {
            setJobs(jobsResponse.data.items);
            setNextCursor(jobsResponse.data.next_cursor);
            const focus = searchParams.get("focus");
            if (focus) setSelectedJob(jobsResponse.data.items.find((job) => job.id === focus) ?? null);
        }
        if (serversResponse.success) setInstances(serversResponse.data);
        if (auditResponse?.success) {
            setAudit(auditResponse.data.items);
            setNextAuditId(auditResponse.data.next_before_id);
        }
        setLoading(false);
    }, [activeTab, canReadAudit, instanceFilter, searchParams, stateFilter]);

    useEffect(() => { void reload(); }, [reload]);
    const { isConnected } = useGlobalEvents({
        enabled: true,
        eventTypes: ["job.updated", "job.waiting_for_user", "server.state", "config.change"],
        onEvent: () => { void reload(); },
        onResynchronize: reload,
    });

    const instanceNames = useMemo(() => new Map(instances.map((instance) => [instance.id, instance.name])), [instances]);
    const attentionJobs = useMemo(() => jobs.filter((job) => job.state === "waiting_for_user"
        || job.state === "failed" || job.state === "interrupted"), [jobs]);
    const crashed = useMemo(() => instances.filter((instance) => instance.runtime_state === "crashed"), [instances]);

    const switchTab = (tab: ActivityTab) => {
        const next = new URLSearchParams(searchParams);
        next.set("tab", tab);
        next.delete("focus");
        setSearchParams(next, { replace: true });
        setSelectedJob(null);
    };

    const updateFilter = (key: "state" | "instance", value: string) => {
        const next = new URLSearchParams(searchParams);
        if (value === "all") next.delete(key); else next.set(key, value);
        next.delete("focus");
        setSearchParams(next, { replace: true });
    };

    const selectJob = (job: Job) => {
        setSelectedJob(job);
        const next = new URLSearchParams(searchParams);
        next.set("focus", job.id);
        setSearchParams(next, { replace: true });
    };

    const closeDetails = () => {
        setSelectedJob(null);
        const next = new URLSearchParams(searchParams);
        next.delete("focus");
        setSearchParams(next, { replace: true });
    };

    const cancelJob = async (job: Job) => {
        if (!await confirm(t("jobs.cancel_confirm"), { title: t("jobs.cancel"), confirmLabel: t("jobs.cancel"), isDestructive: true })) return;
        setCancelling(true);
        const response = await apiService.jobs.cancel(job.id);
        setCancelling(false);
        if (!response.success) return toast.error(response.error.message);
        setJobs((current) => current.map((item) => item.id === job.id ? response.data : item));
        setSelectedJob(response.data);
        toast.success(t("jobs.cancelled"));
    };

    const loadMoreJobs = async () => {
        if (!nextCursor) return;
        const response = await apiService.activity.jobs({ cursor: nextCursor, limit: 50, state: stateFilter ?? undefined, instance_id: instanceFilter });
        if (!response.success) return setError(response.error.message);
        setJobs((current) => [...current, ...response.data.items]);
        setNextCursor(response.data.next_cursor);
    };

    const loadMoreAudit = async () => {
        if (!nextAuditId) return;
        const response = await apiService.activity.audit({ before_id: nextAuditId, limit: 50 });
        if (!response.success) return setError(response.error.message);
        setAudit((current) => [...current, ...response.data.items]);
        setNextAuditId(response.data.next_before_id);
    };

    const locale = language === "fr" ? "fr-FR" : "en-US";
    const hytaleAuthorization = HytaleDeviceAuthorizationSchema.safeParse(selectedJob?.interaction
        ? { job_id: selectedJob.id, interaction: selectedJob.interaction } : null);
    const bedrockAuthorization = BedrockArchiveAuthorizationSchema.safeParse(selectedJob?.interaction
        ? { job_id: selectedJob.id, interaction: selectedJob.interaction } : null);

    return (
        <section className="activity-page">
            <div className="activity-toolbar card">
                <div className="activity-tabs" role="tablist" aria-label={t("activity.title")}>
                    <button role="tab" aria-selected={activeTab === "attention"} className={activeTab === "attention" ? "active" : ""} onClick={() => switchTab("attention")}><CircleAlert size={17} />{t("activity.attention")}{(summary.waiting_for_user + summary.failed_jobs_24h + summary.crashed_servers + summary.config_conflicts) > 0 && <span className="activity-count">{summary.waiting_for_user + summary.failed_jobs_24h + summary.crashed_servers + summary.config_conflicts}</span>}</button>
                    <button role="tab" aria-selected={activeTab === "operations"} className={activeTab === "operations" ? "active" : ""} onClick={() => switchTab("operations")}><ListChecks size={17} />{t("activity.operations")}</button>
                    {canReadAudit && <button role="tab" aria-selected={activeTab === "journal"} className={activeTab === "journal" ? "active" : ""} onClick={() => switchTab("journal")}><History size={17} />{t("activity.journal")}</button>}
                </div>
                <div className="activity-toolbar__status"><span className={`connection-dot ${isConnected ? "connection-dot--online" : ""}`} />{isConnected ? t("realtime.connected") : t("realtime.reconnecting")}<Button size="sm" variant="ghost" aria-label={t("common.refresh")} onClick={() => void reload()}><RefreshCw size={16} /></Button></div>
            </div>

            {error && <div className="alert alert--error" role="alert">{error}</div>}
            {loading ? <div className="card operations-loading">{t("common.loading")}</div> : activeTab === "attention" ? (
                <div className="attention-list">
                    {attentionJobs.map((job) => <button key={job.id} className="attention-row card" onClick={() => selectJob(job)}><AlertTriangle size={19} /><span><strong>{t(`jobs.kinds.${job.kind}`)}</strong><small>{job.instance_id ? instanceNames.get(job.instance_id) ?? t("jobs.unknown_instance") : t("common.system")}</small></span><span className={`badge badge--${badgeVariant(job.state)}`}>{t(`jobs.states.${job.state}`)}</span><ChevronRight size={18} /></button>)}
                    {crashed.map((server) => <Link key={server.id} className="attention-row card" to={`/servers/${server.id}`}><AlertTriangle size={19} /><span><strong>{server.name}</strong><small>{t("activity.server_crashed")}</small></span><span className="badge badge--danger">{t("servers.runtime_states.crashed")}</span><ExternalLink size={17} /></Link>)}
                    {summary.config_conflicts > 0 && <div className="attention-row card"><CircleAlert size={19} /><span><strong>{t("activity.config_conflicts")}</strong><small>{t("activity.config_conflicts_hint")}</small></span><span className="badge badge--warning">{summary.config_conflicts}</span></div>}
                    {attentionJobs.length === 0 && crashed.length === 0 && summary.config_conflicts === 0 && <div className="activity-empty card"><ShieldCheck size={32} /><strong>{t("activity.nothing_to_handle")}</strong><p>{t("activity.nothing_to_handle_hint")}</p></div>}
                </div>
            ) : activeTab === "operations" ? (
                <>
                    <div className="activity-filters card">
                        <label>{t("jobs.filter")}<select className="input" value={stateFilter ?? "all"} onChange={(event) => updateFilter("state", event.target.value)}><option value="all">{t("jobs.all_states")}</option>{["queued", "running", "waiting_for_user", "succeeded", "failed", "cancelled", "interrupted"].map((state) => <option key={state} value={state}>{t(`jobs.states.${state}`)}</option>)}</select></label>
                        <label>{t("jobs.instance_filter")}<select className="input" value={instanceFilter ?? "all"} onChange={(event) => updateFilter("instance", event.target.value)}><option value="all">{t("jobs.all_instances")}</option>{instances.map((instance) => <option key={instance.id} value={instance.id}>{instance.name}</option>)}</select></label>
                    </div>
                    <div className="operation-table card">
                        {jobs.map((job) => <button key={job.id} className="operation-row" onClick={() => selectJob(job)}>
                            <span className={`badge badge--${badgeVariant(job.state)}`}>{t(`jobs.states.${job.state}`)}</span>
                            <span className="operation-row__main"><strong>{t(`jobs.kinds.${job.kind}`) === `jobs.kinds.${job.kind}` ? job.kind : t(`jobs.kinds.${job.kind}`)}</strong><small>{job.instance_id ? instanceNames.get(job.instance_id) ?? t("jobs.unknown_instance") : t("common.system")}</small></span>
                            {ACTIVE_STATES.has(job.state) ? <progress max={100} value={job.progress} aria-label={`${job.progress}%`} /> : <span />}
                            <time dateTime={job.created_at}>{new Date(job.created_at).toLocaleString(locale)}</time>
                            <ChevronRight size={17} />
                        </button>)}
                        {jobs.length === 0 && <div className="activity-empty"><ListChecks size={30} /><p>{t("jobs.empty")}</p></div>}
                    </div>
                    {nextCursor && <div className="activity-load-more"><Button variant="secondary" onClick={() => void loadMoreJobs()}>{t("activity.load_more")}</Button></div>}
                </>
            ) : (
                <div className="audit-table card">
                    <div className="audit-row audit-row--header"><span>{t("activity.date")}</span><span>{t("activity.actor")}</span><span>{t("activity.action")}</span><span>{t("activity.target")}</span><span>{t("activity.result")}</span></div>
                    {audit.map((event) => <div key={event.id} className="audit-row"><time dateTime={event.created_at}>{new Date(event.created_at).toLocaleString(locale)}</time><span>{event.actor_username ?? t("common.system")}</span><code>{event.action}</code><span>{event.resource_type}{event.resource_id ? ` · ${event.resource_id.slice(0, 8)}` : ""}</span><span className={`badge badge--${event.outcome === "success" ? "success" : event.outcome === "denied" ? "warning" : "danger"}`}>{t(`activity.outcomes.${event.outcome}`)}</span></div>)}
                    {nextAuditId && <div className="activity-load-more"><Button variant="secondary" onClick={() => void loadMoreAudit()}>{t("activity.load_more")}</Button></div>}
                </div>
            )}

            {selectedJob && <div className="activity-drawer-backdrop" onMouseDown={(event) => event.target === event.currentTarget && closeDetails()}><aside className="activity-drawer" role="dialog" aria-modal="true" aria-labelledby="activity-detail-title">
                <header><div><span className={`badge badge--${badgeVariant(selectedJob.state)}`}>{t(`jobs.states.${selectedJob.state}`)}</span><h2 id="activity-detail-title">{t(`jobs.kinds.${selectedJob.kind}`)}</h2></div><Button variant="ghost" size="icon" aria-label={t("common.close")} onClick={closeDetails}><X size={19} /></Button></header>
                <dl><div><dt>{t("jobs.instance_filter")}</dt><dd>{selectedJob.instance_id ? instanceNames.get(selectedJob.instance_id) ?? selectedJob.instance_id : t("common.system")}</dd></div><div><dt>{t("jobs.created")}</dt><dd>{new Date(selectedJob.created_at).toLocaleString(locale)}</dd></div><div><dt>{t("jobs.progress")}</dt><dd>{selectedJob.progress}%</dd></div><div><dt>ID</dt><dd><code>{selectedJob.id}</code></dd></div></dl>
                {ACTIVE_STATES.has(selectedJob.state) && <progress max={100} value={selectedJob.progress} />}
                {hytaleAuthorization.success && <HytaleDeviceAuthorizationNotice authorization={hytaleAuthorization.data} />}
                {bedrockAuthorization.success && <BedrockArchiveUploadNotice authorization={bedrockAuthorization.data} canUpload={user?.role === "owner" && hasPermission("server.files.write")} onAccepted={(job) => { setSelectedJob(job); setJobs((items) => items.map((item) => item.id === job.id ? job : item)); }} />}
                {(selectedJob.error_code || selectedJob.error_message) && <div className="alert alert--error"><strong>{selectedJob.error_code}</strong><span>{selectedJob.error_message}</span></div>}
                <footer>{selectedJob.instance_id && <Button as="link" to={selectedJob.kind === "install" ? `/servers/${selectedJob.instance_id}?tab=console&source=install&job=${selectedJob.id}` : `/servers/${selectedJob.instance_id}`} variant="secondary"><ExternalLink size={16} />{t(selectedJob.kind === "install" ? "jobs.view_install_terminal" : "jobs.view_instance")}</Button>}{selectedJob.kind === "install" && ACTIVE_STATES.has(selectedJob.state) && hasPermission("server.update_game") && <Button variant="danger" isLoading={cancelling} onClick={() => void cancelJob(selectedJob)}><Ban size={16} />{t("jobs.cancel")}</Button>}</footer>
            </aside></div>}
        </section>
    );
}
