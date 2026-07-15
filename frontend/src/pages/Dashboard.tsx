import { Activity, Plus, Server as ServerIcon, Square } from "lucide-react";
import { useEffect } from "react";
import { Link } from "react-router-dom";
import { EmptyState, LoadingScreen } from "@/components/shared";
import { ServerFilters, ServerList } from "@/components/features/server";
import { StatPill } from "@/components/ui";
import { useFilteredServers, usePermission } from "@/hooks";
import { useLanguage } from "@/contexts/LanguageContext";
import { usePageTitle } from "@/contexts/PageTitleContext";
import { ServerAction } from "@/services/api/server.client";

export default function Dashboard() {
    const { t } = useLanguage();
    const { setPageTitle } = usePageTitle();
    const { hasPermission } = usePermission();
    const canCreate = hasPermission("server.create");
    const {
        servers,
        profiles,
        loading,
        error,
        stats,
        profileIds,
        search,
        setSearch,
        gameType,
        setGameType,
        viewMode,
        setViewMode,
        handleServerAction,
    } = useFilteredServers({ initialViewMode: "list" });

    useEffect(() => setPageTitle(t("sidebar.dashboard"), t("dashboard.welcome")), [setPageTitle, t]);

    if (loading) return <LoadingScreen />;

    const onAction = (id: string, action: ServerAction) => handleServerAction(action, id);

    return (
        <div className="dashboard-page">
            <div className="dashboard-header-stats">
                <StatPill icon={<ServerIcon size={16} />} label={t("dashboard.total_servers")} value={stats.total} variant="default" />
                <StatPill icon={<Activity size={16} />} label={t("dashboard.running")} value={stats.online} variant="success" />
                <StatPill icon={<Square size={16} />} label={t("dashboard.stopped")} value={stats.offline} variant="muted" />
            </div>

            {error && <div className="alert alert--error" role="alert">{error}</div>}

            <section className="dashboard-servers-section">
                <ServerFilters
                    search={search}
                    onSearchChange={setSearch}
                    gameType={gameType}
                    onGameTypeChange={setGameType}
                    viewMode={viewMode}
                    onViewModeChange={setViewMode}
                    gameTypes={profileIds}
                    action={canCreate ? <Link to="/servers/create" className="btn btn--primary"><Plus size={18} />{t("servers.create_new")}</Link> : undefined}
                />
                {servers.length === 0 ? (
                    <EmptyState
                        icon={<ServerIcon size={48} />}
                        title={t("servers.no_servers")}
                        description={search || gameType !== "all" ? t("dashboard.no_filter_match") : t("servers.empty_desc")}
                        action={canCreate && !search && gameType === "all" ? <Link to="/servers/create" className="btn btn--primary"><Plus size={18} />{t("servers.create_new")}</Link> : undefined}
                    />
                ) : <ServerList servers={servers} profiles={profiles} viewMode={viewMode} onAction={onAction} />}
            </section>
        </div>
    );
}
