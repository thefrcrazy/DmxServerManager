import { Activity, Archive, CalendarClock, Copy, Download, Eye, EyeOff, FolderOpen, Globe2, ListChecks, PackageCheck, Play, Puzzle, RefreshCw, RotateCw, Save, Server as ServerIcon, Skull, Square, Terminal, TriangleAlert, Trash2, Users, Wrench } from "lucide-react";
import { ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router-dom";
import { BedrockArchiveUploadNotice, HytaleDeviceAuthorizationNotice, ProfileConfigurationOverview, ProfileSettingsFields, ServerBackups, ServerConfigFiles, ServerConsole, ServerFiles, ServerMetrics, ServerMods, ServerPlayers, ServerSchedules, profileSettingTitle } from "@/components/features/server";
import { EmptyState, LoadingScreen } from "@/components/shared";
import { Button, StatPill, Tabs } from "@/components/ui";
import { useDialog } from "@/contexts/DialogContext";
import { useAuth } from "@/contexts/AuthContext";
import { useLanguage } from "@/contexts/LanguageContext";
import { usePageTitle } from "@/contexts/PageTitleContext";
import { useToast } from "@/contexts/ToastContext";
import { usePermission, useServerEvents } from "@/hooks";
import { SecretStatusSchema } from "@/schemas/api";
import type { ConnectionInfo, GameProfile, Instance } from "@/schemas/api";
import { BedrockArchiveAuthorizationSchema, HytaleDeviceAuthorizationSchema } from "@/schemas/operations";
import type { BedrockArchiveAuthorization, HytaleDeviceAuthorization, PlayerSnapshot } from "@/schemas/operations";
import { apiService } from "@/services";
import type { ServerAction } from "@/services/api/server.client";
import type { ProfileValue } from "@/utils/profileSettings";
import { gameProfileVisual } from "@/constants/gameProfiles";

type TabId = "configuration" | "console" | "players" | "files" | "backups" | "metrics" | "mods" | "schedules";

export default function ServerDetail() {
    const { id } = useParams<{ id: string }>();
    const navigate = useNavigate();
    const [searchParams, setSearchParams] = useSearchParams();
    const { t } = useLanguage();
    const { setPageTitle } = usePageTitle();
    const toast = useToast();
    const { user } = useAuth();
    const { confirm } = useDialog();
    const { hasPermission } = usePermission();
    const [instance, setInstance] = useState<Instance | null>(null);
    const [profile, setProfile] = useState<GameProfile | null>(null);
    const [settings, setSettings] = useState<Record<string, ProfileValue>>({});
    const [secretStatuses, setSecretStatuses] = useState<Array<{ name: string; configured: boolean }>>([]);
    const [secretDrafts, setSecretDrafts] = useState<Record<string, string>>({});
    const [name, setName] = useState("");
    const [autoStart, setAutoStart] = useState(false);
    const [watchdog, setWatchdog] = useState(true);
    const [activeTab, setActiveTab] = useState<TabId>("configuration");
    const [loading, setLoading] = useState(true);
    const [saving, setSaving] = useState(false);
    const [action, setAction] = useState<ServerAction | null>(null);
    const [loadError, setLoadError] = useState<string | null>(null);
    const [fallbackBedrockArchive, setFallbackBedrockArchive] = useState<BedrockArchiveAuthorization | null>(null);
    const [fallbackDeviceAuthorization, setFallbackDeviceAuthorization] = useState<HytaleDeviceAuthorization | null>(null);
    const [profileOptions, setProfileOptions] = useState<Record<string, readonly string[]>>({});
    const [catalogLoading, setCatalogLoading] = useState(false);
    const [playerSnapshot, setPlayerSnapshot] = useState<PlayerSnapshot | null>(null);
    const [playersLoading, setPlayersLoading] = useState(true);
    const [playersError, setPlayersError] = useState<string | null>(null);
    const [connection, setConnection] = useState<ConnectionInfo | null>(null);
    const [connectionError, setConnectionError] = useState<string | null>(null);
    const [connectionRevealed, setConnectionRevealed] = useState(false);

    const loadInstance = useCallback(async () => {
        if (!id) return;
        const response = await apiService.servers.getServer(id);
        if (!response.success) {
            setLoadError(response.error.message);
            return;
        }
        setInstance(response.data);
        setName(response.data.name);
        setSettings(response.data.settings as Record<string, ProfileValue>);
        setAutoStart(response.data.auto_start);
        setWatchdog(response.data.watchdog_enabled);
        setLoadError(null);
    }, [id]);

    const loadPlayers = useCallback(async () => {
        if (!id) return;
        const response = await apiService.players.snapshot(id);
        setPlayersLoading(false);
        if (!response.success) {
            setPlayersError(response.error.message);
            return;
        }
        setPlayerSnapshot(response.data);
        setPlayersError(null);
    }, [id]);

    const loadConnection = useCallback(async () => {
        if (!id) return;
        const response = await apiService.servers.getConnection(id);
        if (!response.success) {
            setConnectionError(response.error.message);
            return;
        }
        setConnection(response.data);
        setConnectionError(null);
    }, [id]);

    useEffect(() => {
        void loadInstance().finally(() => setLoading(false));
        void loadConnection();
    }, [loadConnection, loadInstance]);

    useEffect(() => {
        if (!instance) return;
        setPageTitle(instance.name, gameProfileVisual(instance.profile_id).label, { to: "/servers" });
        let active = true;
        void apiService.profiles.getRevisions(instance.profile_id).then(async (response) => {
            if (!active) return;
            if (!response.success) {
                const fallback = await apiService.profiles.getProfiles();
                if (active && fallback.success) {
                    setProfile(fallback.data.find((item) => item.id === instance.profile_id) ?? null);
                }
                return;
            }
            const revisions = [...response.data].sort((left, right) => left.revision - right.revision);
            const pinned = revisions.find((item) => item.revision === instance.profile_revision);
            const latest = revisions.at(-1);
            const compatible = latest
                && latest.revision > instance.profile_revision
                && Array.isArray(latest.ui_schema.compatible_from)
                && latest.ui_schema.compatible_from.includes(instance.profile_revision);
            setProfile((compatible ? latest : pinned ?? latest) ?? null);
        });
        return () => { active = false; };
    }, [instance, setPageTitle, t]);

    useEffect(() => {
        if (!id || !profile || !Object.values(profile.settings_schema.properties).some((property) => property.secret || property.writeOnly)) return;
        void apiService.servers.getSecrets(id).then((response) => {
            if (response.success) setSecretStatuses(response.data.items.map((item) => SecretStatusSchema.parse(item)));
        });
    }, [id, profile]);

    const configuredGameVersion = settings.version;
    const configuredLoader = settings.loader;
    const configuredLoaderVersion = settings.loader_version;
    const hasConfiguredLoaderVersion = Object.hasOwn(settings, "loader_version");

    useEffect(() => {
        const usesCatalog = profile?.id === "minecraft-bedrock"
            || profile?.id === "minecraft-java"
            || profile?.id.startsWith("minecraft-java-");
        if (!profile || !usesCatalog) {
            setProfileOptions({});
            setCatalogLoading(false);
            return;
        }
        let active = true;
        const gameVersion = typeof configuredGameVersion === "string" ? configuredGameVersion : undefined;
        const loaderVersion = typeof configuredLoaderVersion === "string"
            ? configuredLoaderVersion
            : "";
        const loader = profile.id === "minecraft-java" && typeof configuredLoader === "string"
            ? configuredLoader
            : undefined;
        const loaderNeedsVersion = loader
            ? ["fabric", "forge", "neoforge", "purpur", "quilt"].includes(loader)
            : Boolean(profile.settings_schema.properties.loader_version);
        setCatalogLoading(true);
        void apiService.profiles.getVersionCatalog(profile.id, gameVersion, loader).then((response) => {
            if (!active || !response.success) return;
            setProfileOptions({
                version: gameVersion && !response.data.game_versions.includes(gameVersion)
                    ? [gameVersion, ...response.data.game_versions]
                    : response.data.game_versions,
                ...(loaderNeedsVersion
                    ? {
                        loader_version: loaderVersion
                            && !response.data.loader_versions.includes(loaderVersion)
                            ? [loaderVersion, ...response.data.loader_versions]
                            : response.data.loader_versions,
                    }
                    : {}),
            });
            if (!loaderNeedsVersion && hasConfiguredLoaderVersion) {
                setSettings((current) => {
                    const next = { ...current };
                    delete next.loader_version;
                    return next;
                });
            }
        }).finally(() => {
            if (active) setCatalogLoading(false);
        });
        return () => {
            active = false;
        };
    }, [configuredGameVersion, configuredLoader, configuredLoaderVersion, hasConfiguredLoaderVersion, profile]);

    const onStatusChange = useCallback((runtimeState: string) => {
        setInstance((current) => current ? { ...current, runtime_state: runtimeState as Instance["runtime_state"] } : current);
    }, []);
    const installationInProgress = instance ? ["installing", "updating"].includes(instance.installation_state) : false;
    const logSource = installationInProgress || searchParams.get("source") === "install" ? "install" : "console";
    const events = useServerEvents({
        serverId: id,
        serverStatus: instance?.runtime_state,
        logSource,
        onServerUpdate: loadInstance,
        onStatusChange,
    });

    useEffect(() => {
        setPlayersLoading(true);
        void loadPlayers();
    }, [events.playerRevision, loadPlayers]);

    useEffect(() => {
        if (!id
            || !instance
            || !["installing", "updating"].includes(instance.installation_state)
            || !hasPermission("job.read")) {
            setFallbackBedrockArchive(null);
            setFallbackDeviceAuthorization(null);
            return;
        }
        let active = true;
        void apiService.jobs.list().then((response) => {
            if (!active || !response.success) return;
            const waitingJob = response.data.find((job) => job.instance_id === id && job.kind === "install" && job.state === "waiting_for_user");
            if (!waitingJob?.interaction) {
                setFallbackBedrockArchive(null);
                setFallbackDeviceAuthorization(null);
                return;
            }
            if (waitingJob.interaction.kind === "bedrock_archive_upload") {
                const parsed = BedrockArchiveAuthorizationSchema.safeParse({ job_id: waitingJob.id, interaction: waitingJob.interaction });
                setFallbackBedrockArchive(parsed.success ? parsed.data : null);
                setFallbackDeviceAuthorization(null);
            } else {
                const parsed = HytaleDeviceAuthorizationSchema.safeParse({ job_id: waitingJob.id, interaction: waitingJob.interaction });
                setFallbackDeviceAuthorization(parsed.success ? parsed.data : null);
                setFallbackBedrockArchive(null);
            }
        });
        return () => { active = false; };
    }, [events.operationRevision, hasPermission, id, instance]);

    const tabs = useMemo(() => {
        const result: Array<{ id: TabId; label: string; icon: ReactNode }> = [
            { id: "configuration", label: t("server_detail.tabs.config"), icon: <Wrench size={18} /> },
        ];
        if (profile?.capabilities.includes("console") && hasPermission("server.console.read")) {
            result.unshift({ id: "console", label: t("server_detail.tabs.terminal"), icon: <Terminal size={18} /> });
        }
        if (hasPermission("server.read")) {
            result.push({ id: "players", label: t("server_detail.tabs.players"), icon: <Users size={18} /> });
        }
        if (profile?.capabilities.includes("files") && hasPermission("server.files.read")) {
            result.push({ id: "files", label: t("server_detail.tabs.files"), icon: <FolderOpen size={18} /> });
        }
        if (profile?.capabilities.includes("backups") && hasPermission("server.backup.read")) {
            result.push({ id: "backups", label: t("server_detail.tabs.backups"), icon: <Archive size={18} /> });
        }
        if (profile?.capabilities.includes("metrics") && hasPermission("server.read")) {
            result.push({ id: "metrics", label: t("server_detail.tabs.metrics"), icon: <Activity size={18} /> });
        }
        const minecraftLoader = profile?.id === "minecraft-java"
            ? instance?.settings.loader
            : undefined;
        const modsAvailable = profile?.capabilities.includes("mods")
            && (profile.id !== "minecraft-java"
                || (typeof minecraftLoader === "string" && minecraftLoader !== "vanilla"));
        if (modsAvailable && hasPermission("mods.manage")) {
            result.push({ id: "mods", label: t("server_detail.tabs.mods"), icon: <Puzzle size={18} /> });
        }
        const supportsScheduledActions = profile?.capabilities.some((capability) =>
            ["lifecycle", "backups", "install", "console"].includes(capability));
        if (profile && supportsScheduledActions && hasPermission("schedule.manage")) {
            result.push({ id: "schedules", label: t("server_detail.tabs.schedules"), icon: <CalendarClock size={18} /> });
        }
        return result;
    }, [hasPermission, instance?.settings.loader, profile, t]);

    useEffect(() => {
        if (!tabs.some((tab) => tab.id === activeTab)) setActiveTab("configuration");
    }, [activeTab, tabs]);

    useEffect(() => {
        const requestedTab = searchParams.get("tab");
        if (requestedTab && tabs.some((tab) => tab.id === requestedTab)) {
            setActiveTab(requestedTab as TabId);
        }
    }, [searchParams, tabs]);

    const runAction = async (nextAction: ServerAction) => {
        if (!id) return;
        setAction(nextAction);
        const response = await apiService.servers.runAction(id, nextAction);
        setAction(null);
        if (!response.success) {
            toast.error(response.error.message);
            return;
        }
        toast.success(t("server_detail.job_queued"));
        if (nextAction === "install") {
            events.clearLogs();
            if (profile?.capabilities.includes("console") && hasPermission("server.console.read")) {
                setActiveTab("console");
                const next = new URLSearchParams(searchParams);
                next.set("tab", "console");
                next.set("job", response.data.id);
                next.set("source", "install");
                setSearchParams(next, { replace: true });
            }
            await loadInstance();
            return;
        }
        const next = new URLSearchParams(searchParams);
        next.delete("source");
        next.delete("job");
        if (["start", "restart"].includes(nextAction)
            && profile?.capabilities.includes("console")
            && hasPermission("server.console.read")) {
            events.clearLogs();
            setActiveTab("console");
            next.set("tab", "console");
        }
        setSearchParams(next, { replace: true });
        await loadInstance();
    };

    const saveConfiguration = async () => {
        if (!id || !instance) return;
        setSaving(true);
        const response = await apiService.servers.updateServer(id, {
            name: name.trim(),
            settings,
            auto_start: autoStart,
            watchdog_enabled: watchdog,
        }, instance.config_version);
        if (!response.success) {
            toast.error(response.error.status === 409 ? t("server_detail.config_conflict") : response.error.message);
            setSaving(false);
            return;
        }

        for (const [secretName, value] of Object.entries(secretDrafts)) {
            if (!value) continue;
            const secretResponse = await apiService.servers.setSecret(id, secretName, value);
            if (!secretResponse.success) {
                toast.error(secretResponse.error.message);
                setSaving(false);
                return;
            }
        }
        setInstance(response.data);
        void loadConnection();
        setSecretDrafts({});
        const statuses = await apiService.servers.getSecrets(id);
        if (statuses.success) setSecretStatuses(statuses.data.items);
        toast.success(t("server_detail.messages.save_success"));
        setSaving(false);
    };

    const deleteInstance = async () => {
        if (!id || !instance) return;
        const accepted = await confirm(t("server_detail.delete_confirm"), {
            isDestructive: true,
            verificationString: instance.name,
            verificationLabel: instance.name,
        });
        if (!accepted) return;
        const response = await apiService.servers.deleteServer(id);
        if (!response.success) return toast.error(response.error.message);
        navigate("/servers");
    };

    if (loading) return <LoadingScreen />;
    if (!instance || loadError) {
        return <EmptyState icon={<ServerIcon size={48} />} title={t("servers.not_found")} description={loadError ?? t("servers.not_found")} />;
    }

    const running = instance.runtime_state === "running";
    const installed = instance.installation_state === "installed";
    const canInstall = profile?.capabilities.includes("install") ?? false;
    const canStartStop = profile?.capabilities.includes("lifecycle") ?? false;
    const busy = action !== null || ["starting", "stopping"].includes(instance.runtime_state)
        || ["installing", "updating"].includes(instance.installation_state);
    const canCancelDesiredRun = installed
        && canStartStop
        && instance.desired_state === "running"
        && ["crashed", "unknown"].includes(instance.runtime_state);
    const fullyStopped = instance.runtime_state === "stopped"
        && instance.desired_state === "stopped"
        && !installationInProgress;
    const configurationDirty = name.trim() !== instance.name
        || autoStart !== instance.auto_start
        || watchdog !== instance.watchdog_enabled
        || JSON.stringify(settings) !== JSON.stringify(instance.settings)
        || Object.values(secretDrafts).some((value) => value.length > 0);
    const bedrockArchive = events.pendingBedrockArchive ?? fallbackBedrockArchive;
    const deviceAuthorization = events.pendingDeviceAuthorization ?? fallbackDeviceAuthorization;
    const canUploadBedrockArchive = user?.role === "owner" && hasPermission("server.files.write");
    const primaryConnection = connection?.endpoints.find((endpoint) => endpoint.primary) ?? connection?.endpoints[0];
    const connectionHelpKey = connection?.help_key.replace(/^connection\.help\./, "") ?? "generic";
    const copyConnection = async () => {
        if (!primaryConnection?.address) return;
        try {
            await navigator.clipboard.writeText(primaryConnection.address);
            toast.success(t("server_detail.connection.copied"));
        } catch {
            toast.error(t("server_detail.connection.copy_failed"));
        }
    };

    return (
        <div className="server-detail-page">
            {deviceAuthorization && <HytaleDeviceAuthorizationNotice authorization={deviceAuthorization} />}
            {bedrockArchive && <BedrockArchiveUploadNotice
                authorization={bedrockArchive}
                canUpload={canUploadBedrockArchive}
                onAccepted={() => {
                    events.clearPendingBedrockArchive();
                    setFallbackBedrockArchive(null);
                    void loadInstance();
                }}
            />}
            <div className="server-header-stats">
                <StatPill icon={<ServerIcon size={18} />} label={t("server_detail.runtime_state")} value={t(`servers.runtime_states.${instance.runtime_state}`)} variant={running ? "success" : instance.runtime_state === "crashed" ? "danger" : "muted"} />
                <StatPill icon={<Users size={18} />} label={t("server_detail.players.online")} value={playerSnapshot?.online_count ?? "—"} variant={playerSnapshot?.online_count ? "success" : "muted"} />
                <StatPill icon={<PackageCheck size={18} />} label={t("server_detail.installed_version")} value={instance.installed_version ?? "—"} variant="default" />
                <div className="stat-pill connection-pill">
                    <div className="stat-pill__icon"><Globe2 size={18} /></div>
                    <div className="stat-pill__content">
                        <span className="stat-pill__label">{t("servers.connection")}</span>
                        {connectionError ? <span className="connection-pill__error">{connectionError}</span> : connection?.configured && primaryConnection?.address ? <><span className="connection-pill__value" aria-live="polite">{connectionRevealed ? primaryConnection.address : `••••••:${primaryConnection.port}`}</span><small>{t(`server_detail.connection.help.${connectionHelpKey}`)}</small></> : <span className="connection-pill__missing">{t("server_detail.connection.not_configured")}</span>}
                    </div>
                    {connection?.configured && primaryConnection?.address && <div className="connection-pill__actions"><Button variant="ghost" size="icon" aria-label={connectionRevealed ? t("server_detail.connection.hide") : t("server_detail.connection.reveal")} onClick={() => setConnectionRevealed((value) => !value)}>{connectionRevealed ? <EyeOff size={16} /> : <Eye size={16} />}</Button><Button variant="ghost" size="icon" aria-label={t("server_detail.connection.copy")} onClick={() => void copyConnection()}><Copy size={16} /></Button></div>}
                </div>
            </div>

            {!connection?.configured && hasPermission("panel.network.manage") && <div className="connection-notice"><Globe2 size={18} /><span>{t("server_detail.connection.configure_hint")}</span><Button as="link" to="/administration?tab=network" variant="ghost" size="sm">{t("server_detail.connection.configure")}</Button></div>}

            <div className="server-actions server-detail-actions">
                {hasPermission("job.read") && <Button as="link" to={`/activity?tab=operations&instance=${encodeURIComponent(instance.id)}`} variant="ghost" icon={<ListChecks size={17} aria-hidden="true" />}>{t("server_detail.view_jobs")}</Button>}
                {!installed && canInstall && hasPermission("server.update_game") && <Button onClick={() => void runAction("install")} disabled={busy} icon={<Download size={17} />}>{t("server_detail.install")}</Button>}
                {installed && canInstall && !running && instance.desired_state === "stopped" && hasPermission("server.update_game") && <Button variant="secondary" onClick={() => void runAction("install")} disabled={busy} icon={<RefreshCw size={17} />}>{t("server_detail.update_game")}</Button>}
                {installed && canStartStop && !running && instance.desired_state === "stopped" && hasPermission("server.start") && <Button variant="success" onClick={() => void runAction("start")} disabled={busy} icon={<Play size={17} />}>{t("servers.start")}</Button>}
                {canCancelDesiredRun && hasPermission("server.stop") && <Button variant="secondary" onClick={() => void runAction("stop")} disabled={busy} icon={<Square size={17} />}>{t("server_detail.cancel_watchdog")}</Button>}
                {running && canStartStop && <>
                    {hasPermission("server.start") && hasPermission("server.stop") && <Button variant="secondary" onClick={() => void runAction("restart")} disabled={busy} icon={<RotateCw size={17} />}>{t("servers.restart")}</Button>}
                    {hasPermission("server.stop") && <Button variant="secondary" onClick={() => void runAction("stop")} disabled={busy} icon={<Square size={17} />}>{t("servers.stop")}</Button>}
                    {hasPermission("server.kill") && <Button variant="danger" onClick={() => void runAction("kill")} disabled={busy} icon={<Skull size={17} />}>{t("servers.kill")}</Button>}
                </>}
            </div>

            {instance.runtime_state === "crashed" && (
                <div className="server-crash-notice" role="status">
                    <TriangleAlert size={20} aria-hidden="true" />
                    <div>
                        <strong>{t("server_detail.crash_notice.title")}</strong>
                        <p>{t(instance.desired_state === "running"
                            ? "server_detail.crash_notice.watchdog_pending"
                            : "server_detail.crash_notice.stopped")}</p>
                    </div>
                </div>
            )}

            <Tabs tabs={tabs} activeTab={activeTab} onTabChange={(tab: TabId) => {
                setActiveTab(tab);
                const next = new URLSearchParams(searchParams);
                next.set("tab", tab);
                setSearchParams(next, { replace: true });
            }} idPrefix="server-detail" panelId="server-detail-tabpanel" />
            <div className="tab-content" id="server-detail-tabpanel" role="tabpanel" aria-labelledby={`server-detail-tab-${activeTab}`}>
                {activeTab === "console" ? (
                    <ServerConsole
                        historyKey={instance.id}
                        logs={events.logs}
                        isRunning={running}
                        isInstalling={["installing", "updating"].includes(instance.installation_state)}
                        onSendCommand={(command) => void events.sendCommand(command)}
                    />
                ) : activeTab === "files" ? (
                    <ServerFiles
                        instanceId={instance.id}
                        canWrite={hasPermission("server.files.write")}
                        isStopped={fullyStopped}
                        refreshSignal={events.operationRevision}
                    />
                ) : activeTab === "players" ? (
                    <ServerPlayers
                        instanceId={instance.id}
                        snapshot={playerSnapshot}
                        loading={playersLoading}
                        error={playersError}
                        canReadAccess={hasPermission("server.files.read")}
                        canReadRawAccess={hasPermission("server.config.raw.read")}
                        canWriteRawAccess={hasPermission("server.files.write") && hasPermission("server.config.raw.write")}
                        isRunning={running}
                        refreshSignal={events.operationRevision}
                        onRefresh={() => void loadPlayers()}
                    />
                ) : activeTab === "backups" ? (
                    <ServerBackups
                        instanceId={instance.id}
                        canManage={hasPermission("server.backup")}
                        isStopped={fullyStopped}
                        refreshSignal={events.operationRevision}
                    />
                ) : activeTab === "metrics" ? (
                    <ServerMetrics
                        instanceId={instance.id}
                        isRunning={instance.runtime_state === "running"}
                        refreshSignal={events.operationRevision}
                    />
                ) : activeTab === "mods" ? (
                    <ServerMods
                        instanceId={instance.id}
                        isInstalled={instance.installation_state === "installed"}
                        isStopped={fullyStopped}
                        refreshSignal={events.operationRevision}
                    />
                ) : activeTab === "schedules" && profile ? (
                    <ServerSchedules
                        instanceId={instance.id}
                        capabilities={profile.capabilities}
                        refreshSignal={events.scheduleRevision}
                    />
                ) : (
                    <div className="card profile-configuration-card">
                        {profile ? (
                            <>
                                <ProfileConfigurationOverview profile={profile} values={settings} activeRevision={instance.profile_revision} />
                                <section className="profile-settings-section">
                                    <header>
                                        <h3>{t("server_detail.profile_config.sections.manager.title")}</h3>
                                        <p>{t("server_detail.profile_config.sections.manager.description")}</p>
                                    </header>
                                    <div className="profile-settings-grid">
                                        <div className="form-group">
                                            <label htmlFor="instance-name">{t("server_detail.instance_name")}</label>
                                            <input id="instance-name" className="input" value={name} onChange={(event) => setName(event.target.value)} minLength={1} maxLength={80} />
                                        </div>
                                        <label className="profile-setting-checkbox">
                                            <input type="checkbox" checked={autoStart} onChange={(event) => setAutoStart(event.target.checked)} />
                                            <span><strong>{t("server_detail.auto_start")}</strong><small>{t("server_detail.profile_config.auto_start_hint")}</small></span>
                                        </label>
                                        <label className="profile-setting-checkbox">
                                            <input type="checkbox" checked={watchdog} onChange={(event) => setWatchdog(event.target.checked)} />
                                            <span><strong>{t("server_detail.watchdog")}</strong><small>{t("server_detail.profile_config.watchdog_hint")}</small></span>
                                        </label>
                                    </div>
                                </section>
                                <ProfileSettingsFields
                                    profile={profile}
                                    values={settings}
                                    options={profileOptions}
                                    loadingOptions={catalogLoading ? ["version", "loader_version"] : []}
                                    includeSecrets={false}
                                    grouped
                                    onChange={(key, value) => setSettings((current) => ({ ...current, [key]: value }))}
                                />
                            </>
                        ) : <p className="text-muted">{t("server_detail.profile_unavailable")}</p>}

                        {profile && Object.entries(profile.settings_schema.properties).some(([, property]) => property.secret || property.writeOnly) && (
                            <section className="profile-settings-section">
                                <header>
                                    <h3>{t("server_detail.profile_config.sections.secrets.title")}</h3>
                                    <p>{t("server_detail.profile_config.sections.secrets.description")}</p>
                                </header>
                                <div className="profile-settings-grid">
                                    {Object.entries(profile.settings_schema.properties).filter(([, property]) => property.secret || property.writeOnly).map(([secretName, property]) => {
                                        const configured = secretStatuses.find((item) => item.name === secretName)?.configured ?? false;
                                        return (
                                            <div className="form-group" key={secretName}>
                                                <label htmlFor={`secret-${secretName}`}>{profileSettingTitle(t, secretName, property)} <span className={`badge badge--${configured ? "success" : "muted"}`}>{configured ? t("server_detail.configured") : t("server_detail.not_configured")}</span></label>
                                                <input id={`secret-${secretName}`} className="input" type="password" autoComplete="new-password" value={secretDrafts[secretName] ?? ""} placeholder={configured ? t("server_detail.keep_secret") : t("server_detail.enter_secret")} onChange={(event) => setSecretDrafts((current) => ({ ...current, [secretName]: event.target.value }))} />
                                            </div>
                                        );
                                    })}
                                </div>
                            </section>
                        )}
                        {hasPermission("server.update") && <div className={`configuration-save-panel ${configurationDirty ? "is-dirty" : ""}`} aria-live="polite">
                            <div className="configuration-save-panel__status"><span><Save size={17} aria-hidden="true" /></span><div><strong>{t("server_detail.configuration_actions.title")}</strong><small>{t(configurationDirty ? "server_detail.configuration_actions.unsaved" : "server_detail.configuration_actions.saved")}</small></div></div>
                            <Button onClick={() => void saveConfiguration()} isLoading={saving} disabled={!configurationDirty || name.trim().length === 0} icon={<Save size={17} />}>{t("common.save")}</Button>
                        </div>}
                        {hasPermission("server.files.read") && (
                            <ServerConfigFiles
                                instanceId={instance.id}
                                category="configuration"
                                canReadRaw={hasPermission("server.config.raw.read")}
                                canWriteRaw={hasPermission("server.files.write") && hasPermission("server.config.raw.write")}
                                isRunning={running}
                                refreshSignal={events.operationRevision}
                            />
                        )}
                        {(user?.role === "owner" || user?.role === "admin") && <details className="server-diagnostics"><summary>{t("server_detail.diagnostics.title")}</summary><dl><div><dt>{t("server_detail.diagnostics.profile")}</dt><dd><code>{instance.profile_id} · #{instance.profile_revision}</code></dd></div><div><dt>{t("server_detail.diagnostics.configuration")}</dt><dd><code>{instance.config_version}</code></dd></div><div><dt>{t("server_detail.diagnostics.desired_state")}</dt><dd><code>{instance.desired_state}</code></dd></div>{instance.installed_build && <div><dt>{t("server_detail.diagnostics.build")}</dt><dd><code>{instance.installed_build}</code></dd></div>}</dl></details>}
                        {hasPermission("server.delete") && <section className="instance-danger-zone" aria-labelledby="instance-danger-zone-title">
                            <div><h3 id="instance-danger-zone-title"><TriangleAlert size={17} aria-hidden="true" />{t("server_detail.danger_zone.title")}</h3><p>{t(fullyStopped ? "server_detail.danger_zone.description" : "server_detail.danger_zone.stop_required")}</p></div>
                            <Button variant="danger" onClick={() => void deleteInstance()} disabled={!fullyStopped} icon={<Trash2 size={17} />}>{t("server_detail.danger_zone.delete")}</Button>
                        </section>}
                    </div>
                )}
            </div>
        </div>
    );
}
