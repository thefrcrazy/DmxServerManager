import { AlertTriangle, Activity, CircleCheck, Server as ServerIcon } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { EmptyState, LoadingScreen } from "@/components/shared";
import { StatPill } from "@/components/ui";
import { useLanguage } from "@/contexts/LanguageContext";
import { usePageTitle } from "@/contexts/PageTitleContext";
import { useGlobalEvents, usePermission, useServers } from "@/hooks";
import type { Job } from "@/schemas/api";
import type { ActivitySummary } from "@/schemas/operations";
import { apiService } from "@/services";
import { gameProfileVisual } from "@/constants/gameProfiles";

const EMPTY_SUMMARY: ActivitySummary = {
    active_jobs: 0,
    waiting_for_user: 0,
    failed_jobs_24h: 0,
    crashed_servers: 0,
    config_conflicts: 0,
};

export default function Dashboard() {
    const { t, language } = useLanguage();
    const { setPageTitle } = usePageTitle();
    const { hasPermission } = usePermission();
    const { servers, loading: serversLoading, refresh: refreshServers } = useServers();
    const [summary, setSummary] = useState<ActivitySummary>(EMPTY_SUMMARY);
    const [jobs, setJobs] = useState<Job[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState("");
    const canReadJobs = hasPermission("job.read");

    useEffect(() => setPageTitle(t("sidebar.dashboard"), t("dashboard.operational_subtitle")), [setPageTitle, t]);

    const reload = useCallback(async () => {
        const [summaryResponse, jobsResponse] = await Promise.all([
            apiService.activity.summary(),
            canReadJobs ? apiService.activity.jobs({ limit: 8 }) : Promise.resolve(null),
        ]);
        if (!summaryResponse.success) {
            setError(summaryResponse.error.message);
        } else {
            setSummary(summaryResponse.data);
            setError("");
        }
        if (jobsResponse?.success) setJobs(jobsResponse.data.items);
        setLoading(false);
    }, [canReadJobs]);

    useEffect(() => { void reload(); }, [reload]);
    useGlobalEvents({
        enabled: true,
        eventTypes: ["job.updated", "server.state", "config.change"],
        onEvent: () => { void reload(); void refreshServers(); },
        onResynchronize: async () => { await Promise.all([reload(), refreshServers()]); },
    });

    const running = useMemo(() => servers.filter((server) => server.runtime_state === "running"), [servers]);
    const actionRequired = summary.waiting_for_user + summary.failed_jobs_24h
        + summary.crashed_servers + summary.config_conflicts;

    if (loading && serversLoading) return <LoadingScreen />;

    return (
        <div className="dashboard-page dashboard-page--operational">
            {error && <div className="alert alert--error" role="alert">{error}</div>}

            <section className="dashboard-header-stats" aria-label={t("dashboard.health_overview")}>
                <StatPill icon={<ServerIcon size={17} />} label={t("dashboard.running")} value={running.length} variant="success" />
                <StatPill icon={<AlertTriangle size={17} />} label={t("dashboard.crashed")} value={summary.crashed_servers} variant={summary.crashed_servers ? "danger" : "muted"} />
                <StatPill icon={<Activity size={17} />} label={t("dashboard.action_required")} value={actionRequired} variant={actionRequired ? "warning" : "muted"} />
                <StatPill icon={<CircleCheck size={17} />} label={t("dashboard.operations_running")} value={summary.active_jobs} variant="default" />
            </section>

            <div className="dashboard-operations-grid">
                <section className="card dashboard-panel">
                    <header className="dashboard-panel__header">
                        <div><h2>{t("dashboard.server_health")}</h2><p>{t("dashboard.server_health_hint")}</p></div>
                        <Link to="/servers" className="btn btn--ghost btn--sm">{t("dashboard.view_all_servers")}</Link>
                    </header>
                    {servers.length === 0 ? (
                        <EmptyState icon={<ServerIcon size={36} />} title={t("servers.no_servers")} description={t("servers.empty_desc")} />
                    ) : (
                        <div className="health-list">
                            {servers.slice(0, 8).map((server) => {
                                const visual = gameProfileVisual(server.profile_id);
                                return <Link key={server.id} to={`/servers/${server.id}`} className="health-row">
                                    <span className={`health-row__dot health-row__dot--${server.runtime_state}`} aria-hidden="true" />
                                    <span className="health-row__identity"><strong>{server.name}</strong><small>{visual.label}</small></span>
                                    <span className="health-row__version">{server.installed_version ?? "—"}</span>
                                    <span className={`badge badge--${server.runtime_state === "running" ? "success" : server.runtime_state === "crashed" ? "danger" : "neutral"}`}>{t(`servers.runtime_states.${server.runtime_state}`)}</span>
                                </Link>;
                            })}
                        </div>
                    )}
                </section>

                <section className="card dashboard-panel">
                    <header className="dashboard-panel__header">
                        <div><h2>{t("dashboard.recent_activity")}</h2><p>{t("dashboard.recent_activity_hint")}</p></div>
                        {canReadJobs && <Link to="/activity?tab=operations" className="btn btn--ghost btn--sm">{t("dashboard.open_activity")}</Link>}
                    </header>
                    {jobs.length === 0 ? <p className="dashboard-panel__empty">{t("dashboard.no_recent_activity")}</p> : (
                        <div className="activity-preview-list">
                            {jobs.map((job) => <Link key={job.id} to={`/activity?tab=operations&focus=${job.id}`} className="activity-preview-row">
                                <span className={`badge badge--${job.state === "succeeded" ? "success" : job.state === "failed" || job.state === "interrupted" ? "danger" : job.state === "waiting_for_user" ? "warning" : "info"}`}>{t(`jobs.states.${job.state}`)}</span>
                                <strong>{t(`jobs.kinds.${job.kind}`) === `jobs.kinds.${job.kind}` ? job.kind : t(`jobs.kinds.${job.kind}`)}</strong>
                                <time dateTime={job.created_at}>{new Date(job.created_at).toLocaleString(language === "fr" ? "fr-FR" : "en-US")}</time>
                            </Link>)}
                        </div>
                    )}
                </section>
            </div>
        </div>
    );
}
