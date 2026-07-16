import { useEffect, useCallback } from "react";
import { Link } from "react-router-dom";
import { Plus, Server as ServerIcon } from "lucide-react";
import { useLanguage } from "../contexts/LanguageContext";
import { usePageTitle } from "../contexts/PageTitleContext";
import { ServerList, ServerFilters } from "@/components/features/server";
import { useFilteredServers } from "../hooks";
import { usePermission } from "../hooks";
import { LoadingScreen, EmptyState } from "@/components/shared";
import { ServerAction } from "@/services/api/server.client";

export default function Servers() {
    const { t } = useLanguage();
    const { setPageTitle } = usePageTitle();
    const { hasPermission } = usePermission();
    const canCreate = hasPermission("server.create");

    const {
        servers,
        profiles,
        loading,
        profileIds,
        search,
        setSearch,
        gameType,
        setGameType,
        viewMode,
        setViewMode,
        handleServerAction,
    } = useFilteredServers({ initialViewMode: "grid" });

    useEffect(() => {
        setPageTitle(t("servers.title"), t("dashboard.welcome"), { to: "/" });
    }, [setPageTitle, t]);

    // Adapter function to match ServerList's expected signature
    const onAction = useCallback(async (id: string, action: ServerAction) => {
        return handleServerAction(action, id);
    }, [handleServerAction]);

    if (loading) {
        return <LoadingScreen />;
    }

    return (
        <div className="servers-page">
            <ServerFilters
                search={search}
                onSearchChange={setSearch}
                gameType={gameType}
                onGameTypeChange={setGameType}
                viewMode={viewMode}
                onViewModeChange={setViewMode}
                gameTypes={profileIds}
                action={canCreate ? (
                    <Link to="/servers/create" className="btn btn--primary">
                        <Plus size={18} />
                        {t("servers.create_new")}
                    </Link>
                ) : undefined}
            />

            {servers.length === 0 ? (
                <EmptyState
                    icon={<ServerIcon size={48} />}
                    title={t("servers.no_servers")}
                    description={search || gameType !== "all" ? t("dashboard.no_filter_match") : t("servers.empty_desc")}
                    action={
                        canCreate && (search === "" && gameType === "all") && (
                            <Link to="/servers/create" className="btn btn--primary">
                                <Plus size={18} />
                                {t("servers.create_new")}
                            </Link>
                        )
                    }
                />
            ) : (
                <ServerList
                    servers={servers}
                    profiles={profiles}
                    viewMode={viewMode}
                    onAction={onAction}
                />
            )}
        </div>
    );
}
