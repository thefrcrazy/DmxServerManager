import { Activity, Archive, CalendarClock, Download, FolderOpen, ListChecks, PackageCheck, Play, Puzzle, RefreshCw, RotateCw, Save, Server as ServerIcon, Shield, Skull, Square, Terminal, Trash2, Wrench } from "lucide-react";
import { ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { BedrockArchiveUploadNotice, HytaleDeviceAuthorizationNotice, ProfileSettingsFields, ServerBackups, ServerConsole, ServerFiles, ServerMetrics, ServerMods, ServerSchedules } from "@/components/features/server";
import { EmptyState, LoadingScreen } from "@/components/shared";
import { Button, StatPill, Tabs } from "@/components/ui";
import { useDialog } from "@/contexts/DialogContext";
import { useAuth } from "@/contexts/AuthContext";
import { useLanguage } from "@/contexts/LanguageContext";
import { usePageTitle } from "@/contexts/PageTitleContext";
import { useToast } from "@/contexts/ToastContext";
import { usePermission, useServerEvents } from "@/hooks";
import { SecretStatusSchema } from "@/schemas/api";
import type { GameProfile, Instance } from "@/schemas/api";
import { BedrockArchiveAuthorizationSchema, HytaleDeviceAuthorizationSchema } from "@/schemas/operations";
import type { BedrockArchiveAuthorization, HytaleDeviceAuthorization } from "@/schemas/operations";
import { apiService } from "@/services";
import type { ServerAction } from "@/services/api/server.client";
import type { ProfileValue } from "@/utils/profileSettings";

type TabId = "configuration" | "console" | "files" | "backups" | "metrics" | "mods" | "schedules";

export default function ServerDetail() {
    const { id } = useParams<{ id: string }>();
    const navigate = useNavigate();
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

    useEffect(() => {
        void loadInstance().finally(() => setLoading(false));
    }, [loadInstance]);

    useEffect(() => {
        if (!instance) return;
        setPageTitle(instance.name, `${instance.profile_id} · ${t("servers.revision")} ${instance.profile_revision}`, { to: "/servers" });
        void apiService.profiles.getProfiles().then((response) => {
            if (response.success) setProfile(response.data.find((item) => item.id === instance.profile_id) ?? null);
        });
    }, [instance, setPageTitle, t]);

    useEffect(() => {
        if (!id || !profile || !Object.values(profile.settings_schema.properties).some((property) => property.secret || property.writeOnly)) return;
        void apiService.servers.getSecrets(id).then((response) => {
            if (response.success) setSecretStatuses(response.data.items.map((item) => SecretStatusSchema.parse(item)));
        });
    }, [id, profile]);

    const onStatusChange = useCallback((runtimeState: string) => {
        setInstance((current) => current ? { ...current, runtime_state: runtimeState as Instance["runtime_state"] } : current);
    }, []);
    const events = useServerEvents({ serverId: id, serverStatus: instance?.runtime_state, onServerUpdate: loadInstance, onStatusChange });

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
        if (profile?.capabilities.includes("files") && hasPermission("server.files.read")) {
            result.push({ id: "files", label: t("server_detail.tabs.files"), icon: <FolderOpen size={18} /> });
        }
        if (profile?.capabilities.includes("backups") && hasPermission("server.backup.read")) {
            result.push({ id: "backups", label: t("server_detail.tabs.backups"), icon: <Archive size={18} /> });
        }
        if (profile?.capabilities.includes("metrics") && hasPermission("server.read")) {
            result.push({ id: "metrics", label: t("server_detail.tabs.metrics"), icon: <Activity size={18} /> });
        }
        if (profile?.capabilities.includes("mods") && hasPermission("mods.manage")) {
            result.push({ id: "mods", label: t("server_detail.tabs.mods"), icon: <Puzzle size={18} /> });
        }
        const supportsScheduledActions = profile?.capabilities.some((capability) =>
            ["lifecycle", "backups", "install", "console"].includes(capability));
        if (profile && supportsScheduledActions && hasPermission("schedule.manage")) {
            result.push({ id: "schedules", label: t("server_detail.tabs.schedules"), icon: <CalendarClock size={18} /> });
        }
        return result;
    }, [hasPermission, profile, t]);

    useEffect(() => {
        if (!tabs.some((tab) => tab.id === activeTab)) setActiveTab("configuration");
    }, [activeTab, tabs]);

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
        if (nextAction === "install" && hasPermission("job.read")) {
            navigate(`/jobs?focus=${encodeURIComponent(response.data.id)}&instance=${encodeURIComponent(id)}`);
            return;
        }
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
    const bedrockArchive = events.pendingBedrockArchive ?? fallbackBedrockArchive;
    const deviceAuthorization = events.pendingDeviceAuthorization ?? fallbackDeviceAuthorization;
    const canUploadBedrockArchive = user?.role === "owner" && hasPermission("server.files.write");

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
                <StatPill icon={<Download size={18} />} label={t("server_detail.installation_state")} value={t(`servers.installation_states.${instance.installation_state}`)} variant={installed ? "success" : "warning"} />
                <StatPill icon={<Shield size={18} />} label={t("server_detail.desired_state")} value={t(`servers.desired_states.${instance.desired_state}`)} variant={instance.desired_state === "running" ? "success" : "muted"} />
                <StatPill icon={<Wrench size={18} />} label={t("server_detail.configuration_version")} value={`v${instance.config_version}`} variant="default" />
                {instance.installed_version && <StatPill icon={<PackageCheck size={18} />} label={t("server_detail.installed_version")} value={instance.installed_version} variant="default" />}
                {instance.installed_build && <StatPill icon={<PackageCheck size={18} />} label={t("server_detail.installed_build")} value={instance.installed_build} variant="default" />}
            </div>

            <div className="server-actions" style={{ marginBottom: "1rem", justifyContent: "flex-end" }}>
                {hasPermission("job.read") && <Button as="link" to={`/jobs?instance=${encodeURIComponent(instance.id)}`} variant="ghost" icon={<ListChecks size={17} aria-hidden="true" />}>{t("server_detail.view_jobs")}</Button>}
                {!installed && canInstall && hasPermission("server.update_game") && <Button onClick={() => void runAction("install")} disabled={busy} icon={<Download size={17} />}>{t("server_detail.install")}</Button>}
                {installed && canInstall && !running && hasPermission("server.update_game") && <Button variant="secondary" onClick={() => void runAction("install")} disabled={busy} icon={<RefreshCw size={17} />}>{t("server_detail.update_game")}</Button>}
                {installed && canStartStop && !running && hasPermission("server.start") && <Button variant="success" onClick={() => void runAction("start")} disabled={busy} icon={<Play size={17} />}>{t("servers.start")}</Button>}
                {running && canStartStop && <>
                    {hasPermission("server.start") && hasPermission("server.stop") && <Button variant="secondary" onClick={() => void runAction("restart")} disabled={busy} icon={<RotateCw size={17} />}>{t("servers.restart")}</Button>}
                    {hasPermission("server.stop") && <Button variant="secondary" onClick={() => void runAction("stop")} disabled={busy} icon={<Square size={17} />}>{t("servers.stop")}</Button>}
                    {hasPermission("server.kill") && <Button variant="danger" onClick={() => void runAction("kill")} disabled={busy} icon={<Skull size={17} />}>{t("servers.kill")}</Button>}
                </>}
            </div>

            <Tabs tabs={tabs} activeTab={activeTab} onTabChange={(tab: TabId) => setActiveTab(tab)} idPrefix="server-detail" panelId="server-detail-tabpanel" />
            <div className="tab-content" id="server-detail-tabpanel" role="tabpanel" aria-labelledby={`server-detail-tab-${activeTab}`}>
                {activeTab === "console" ? (
                    <ServerConsole
                        logs={events.logs}
                        isConnected={events.isConnected}
                        isRunning={running}
                        onSendCommand={(command) => void events.sendCommand(command)}
                    />
                ) : activeTab === "files" ? (
                    <ServerFiles
                        instanceId={instance.id}
                        canWrite={hasPermission("server.files.write")}
                        isStopped={instance.runtime_state === "stopped"}
                        refreshSignal={events.operationRevision}
                    />
                ) : activeTab === "backups" ? (
                    <ServerBackups
                        instanceId={instance.id}
                        canManage={hasPermission("server.backup")}
                        isStopped={instance.runtime_state === "stopped"}
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
                        isStopped={instance.runtime_state === "stopped"}
                        refreshSignal={events.operationRevision}
                    />
                ) : activeTab === "schedules" && profile ? (
                    <ServerSchedules
                        instanceId={instance.id}
                        capabilities={profile.capabilities}
                        refreshSignal={events.scheduleRevision}
                    />
                ) : (
                    <div className="card">
                        <div className="form-group">
                            <label htmlFor="instance-name">{t("server_detail.instance_name")}</label>
                            <input id="instance-name" className="input" value={name} onChange={(event) => setName(event.target.value)} minLength={1} maxLength={80} />
                        </div>
                        {profile ? <ProfileSettingsFields
                            profile={profile}
                            values={settings}
                            includeSecrets={false}
                            onChange={(key, value) => setSettings((current) => ({ ...current, [key]: value }))}
                        /> : <p className="text-muted">{t("server_detail.profile_unavailable")} {instance.profile_id}@{instance.profile_revision}</p>}

                        {profile && Object.entries(profile.settings_schema.properties).filter(([, property]) => property.secret || property.writeOnly).map(([secretName, property]) => {
                            const configured = secretStatuses.find((item) => item.name === secretName)?.configured ?? false;
                            return (
                                <div className="form-group" key={secretName}>
                                    <label htmlFor={`secret-${secretName}`}>{property.title ?? secretName} <span className={`badge badge--${configured ? "success" : "muted"}`}>{configured ? t("server_detail.configured") : t("server_detail.not_configured")}</span></label>
                                    <input id={`secret-${secretName}`} className="input" type="password" autoComplete="new-password" value={secretDrafts[secretName] ?? ""} placeholder={configured ? t("server_detail.keep_secret") : t("server_detail.enter_secret")} onChange={(event) => setSecretDrafts((current) => ({ ...current, [secretName]: event.target.value }))} />
                                </div>
                            );
                        })}

                        <label className="form-checkbox"><input type="checkbox" checked={autoStart} onChange={(event) => setAutoStart(event.target.checked)} /><span>{t("server_detail.auto_start")}</span></label>
                        <label className="form-checkbox"><input type="checkbox" checked={watchdog} onChange={(event) => setWatchdog(event.target.checked)} /><span>{t("server_detail.watchdog")}</span></label>
                        <div className="form-footer">
                            {hasPermission("server.delete") && <Button variant="danger" onClick={() => void deleteInstance()} disabled={instance.runtime_state !== "stopped" || instance.desired_state !== "stopped"} icon={<Trash2 size={17} />}>{t("common.delete")}</Button>}
                            <Button onClick={() => void saveConfiguration()} isLoading={saving} disabled={!hasPermission("server.update")} icon={<Save size={17} />}>{t("common.save")}</Button>
                        </div>
                    </div>
                )}
            </div>
        </div>
    );
}
