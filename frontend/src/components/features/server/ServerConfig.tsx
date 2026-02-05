import React from "react";
import { Server as ServerIcon, Terminal, Cpu, Globe, Save, ChevronDown, Check, AlertTriangle, RotateCw, Trash2 } from "lucide-react";
import Checkbox from "@/components/ui/Checkbox";
import RangeSlider from "@/components/ui/RangeSlider";
import Select from "@/components/ui/Select";
import { useLanguage } from "@/contexts/LanguageContext";

interface ServerConfigProps {
    configFormData: any;
    configSaving: boolean;
    configError: string;
    javaVersions: { path: string; version: string }[];
    updateConfigValue: (key: any, value: any) => void;
    toggleJvmArg: (arg: string) => void;
    handleSaveConfig: (e: React.FormEvent) => void;
    onDelete: () => void;
    onReinstall: () => void;
    hasChanges?: boolean;
}

const JVM_ARGS_SUGGESTIONS = [
    { key: "aot", arg: "-XX:AOTCache=HytaleServer.aot", isRecommended: true },
    { key: "g1gc", arg: "-XX:+UseG1GC", isRecommended: false },
    { key: "zgc", arg: "-XX:+UseZGC", isRecommended: false },
    { key: "maxgcpause", arg: "-XX:MaxGCPauseMillis=50", isRecommended: false },
    { key: "parallelref", arg: "-XX:+ParallelRefProcEnabled", isRecommended: false },
    { key: "disableexplicitgc", arg: "-XX:+DisableExplicitGC", isRecommended: false },
    { key: "alwayspretouch", arg: "-XX:+AlwaysPreTouch", isRecommended: false },
    { key: "stringdedup", arg: "-XX:+UseStringDeduplication", isRecommended: false },
    { key: "encoding", arg: "-Dfile.encoding=UTF-8", isRecommended: false },
];

interface CollapsibleSectionProps {
    title: string;
    icon: React.ElementType;
    children: React.ReactNode;
    badge?: string;
    defaultOpen?: boolean;
    className?: string;
}

const CollapsibleSection = ({
    title,
    icon: Icon,
    children,
    badge,
    defaultOpen = false,
    className = ""
}: CollapsibleSectionProps) => (
    <details className={`config-section ${className}`} open={defaultOpen}>
        <summary className="config-section-header">
            <div className="header-left">
                <Icon size={18} className="text-primary" />
                <span className="title">{title}</span>
                {badge && <span className="badge">{badge}</span>}
            </div>
            <ChevronDown size={18} className="chevron" />
        </summary>
        <div className="config-section-content">
            {children}
        </div>
    </details>
);

export default function ServerConfig({
    configFormData,
    configSaving,
    configError,
    javaVersions,
    updateConfigValue,
    toggleJvmArg,
    handleSaveConfig,
    onDelete,
    onReinstall,
    hasChanges = false
}: ServerConfigProps) {
    const { t } = useLanguage();

    return (
        <div className="config-wrapper">
            <form onSubmit={handleSaveConfig} className="config-form">

                {/* Header Action Bar */}
                <div className={`config-action-bar ${hasChanges ? "is-visible" : ""}`}>
                    <div className="action-info">
                        <div className="icon-circle">
                            <Save size={20} />
                        </div>
                        <div className="text-group">
                            <h3>{t("server_detail.config.title")}</h3>
                            <p>{t("server_detail.config.subtitle")}</p>
                        </div>
                    </div>
                    <button
                        type="submit"
                        disabled={configSaving}
                        className="btn btn--primary"
                    >
                        {configSaving ? t("common.saving") : t("common.save")}
                    </button>
                </div>

                {configError && (
                    <div className="alert alert--error">
                        {configError}
                    </div>
                )}

                <div className="config-grid">
                    {/* General Settings */}
                    <div className="grid-full">
                        <CollapsibleSection title={t("server_detail.headers.general")} icon={ServerIcon} defaultOpen={true}>
                            <div className="grid-2">
                                <div className="form-group">
                                    <label>{t("server_detail.config.server_name")}</label>
                                    <input
                                        type="text"
                                        value={configFormData.name || ""}
                                        onChange={(e) => updateConfigValue("name", e.target.value)}
                                        className="input"
                                        placeholder="Mon Serveur Hytale"
                                    />
                                    <p className="helper-text">{t("server_detail.config.server_name_help")}</p>
                                </div>
                                <div className="form-group">
                                    <label>{t("server_detail.config.auth_mode")}</label>
                                    <Select
                                        options={[
                                            { label: "Authenticated (Online Mode)", value: "authenticated" },
                                            { label: "Offline (Insecure)", value: "offline" },
                                        ]}
                                        value={configFormData.auth_mode || "authenticated"}
                                        onChange={(v) => updateConfigValue("auth_mode", v)}
                                    />
                                </div>
                                <div className="form-group grid-full">
                                    <label>MOTD</label>
                                    <input
                                        type="text"
                                        value={configFormData.motd || ""}
                                        onChange={(e) => updateConfigValue("motd", e.target.value)}
                                        className="input"
                                        placeholder="Message of the Day"
                                    />
                                </div>
                                <div className="form-group">
                                    <label>{t("auth.password")}</label>
                                    <input
                                        type="text"
                                        value={configFormData.password || ""}
                                        onChange={(e) => updateConfigValue("password", e.target.value)}
                                        className="input font-mono"
                                        placeholder="(Optional) Server Password"
                                    />
                                </div>
                            </div>
                        </CollapsibleSection>
                    </div>

                    {/* Resources (JVM) */}
                    <div className="grid-full">
                        <CollapsibleSection title={t("server_detail.headers.resources")} icon={Cpu} defaultOpen={true}>
                            <div className="grid-2">
                                <div className="form-column">
                                    <div className="form-group">
                                        <label>{t("server_detail.config.ram_min")}</label>
                                        <input
                                            type="text"
                                            value={configFormData.min_memory || ""}
                                            onChange={(e) => updateConfigValue("min_memory", e.target.value)}
                                            className="input font-mono"
                                            placeholder="ex: 1G"
                                        />
                                    </div>
                                    <div className="form-group">
                                        <label>{t("server_detail.config.ram_max")}</label>
                                        <input
                                            type="text"
                                            value={configFormData.max_memory || ""}
                                            onChange={(e) => updateConfigValue("max_memory", e.target.value)}
                                            className="input font-mono"
                                            placeholder="ex: 4G"
                                        />
                                    </div>
                                </div>
                                <div className="form-column">
                                    <div className="form-group">
                                        <label>{t("server_detail.config.java_version")}</label>
                                        <Select
                                            options={[
                                                { label: t("server_detail.config.system_default"), value: "" },
                                                ...javaVersions.map((j) => ({
                                                    label: `Java ${j.version} (${j.path})`,
                                                    value: j.path,
                                                })),
                                            ]}
                                            value={configFormData.java_path || ""}
                                            onChange={(v) => updateConfigValue("java_path", v)}
                                        />
                                    </div>
                                    <div className="form-group">
                                        <label>{t("server_detail.config.jvm_args")}</label>
                                        <input
                                            type="text"
                                            value={configFormData.extra_args || ""}
                                            onChange={(e) => updateConfigValue("extra_args", e.target.value)}
                                            className="input font-mono"
                                        />
                                    </div>
                                </div>
                            </div>

                            <div className="jvm-suggestions">
                                <label>{t("server_detail.config.recommended_optimizations")}</label>
                                <div className="suggestions-grid">
                                    {JVM_ARGS_SUGGESTIONS.map(({ arg, key }) => (
                                        <button
                                            key={key}
                                            type="button"
                                            onClick={() => toggleJvmArg(arg)}
                                            className={`suggestion-chip ${(configFormData.extra_args || "").includes(arg) ? "active" : ""}`}
                                            title={t(`server_detail.jvm.${key}`) || arg}
                                        >
                                            <div className="check-circle">
                                                {(configFormData.extra_args || "").includes(arg) && <Check size={8} />}
                                            </div>
                                            <span>{arg}</span>
                                        </button>
                                    ))}
                                </div>
                            </div>
                        </CollapsibleSection>
                    </div>

                    {/* Launch Arguments */}
                    <div className="grid-half">
                        <CollapsibleSection title={t("server_detail.headers.launch_args")} icon={Terminal} defaultOpen={true}>
                            <div className="form-column">
                                <div className="form-group">
                                    <label>{t("server_detail.config.ip_address")}</label>
                                    <input
                                        type="text"
                                        value={configFormData.bind_address || "0.0.0.0"}
                                        onChange={(e) => updateConfigValue("bind_address", e.target.value)}
                                        className="input font-mono"
                                    />
                                </div>
                                <div className="form-group">
                                    <label>{t("server_detail.config.port")}</label>
                                    <input
                                        type="number"
                                        value={configFormData.port || 5520}
                                        onChange={(e) => updateConfigValue("port", parseInt(e.target.value))}
                                        className="input font-mono"
                                    />
                                </div>
                                <div className="checkbox-stack">
                                    <Checkbox
                                        checked={configFormData.allow_op || false}
                                        onChange={(v: boolean) => updateConfigValue("allow_op", v)}
                                        label={t("server_detail.config.allow_op")}
                                        description={t("server_detail.config.allow_op_desc")}
                                    />
                                    <Checkbox
                                        checked={configFormData.disable_sentry || false}
                                        onChange={(v: boolean) => updateConfigValue("disable_sentry", v)}
                                        label={t("server_detail.config.disable_sentry")}
                                        description={t("server_detail.config.disable_sentry_desc")}
                                    />
                                    <Checkbox
                                        checked={configFormData.accept_early_plugins || false}
                                        onChange={(v: boolean) => updateConfigValue("accept_early_plugins", v)}
                                        label={t("server_detail.config.early_plugins")}
                                        description={t("server_detail.config.early_plugins_desc")}
                                    />
                                </div>
                            </div>
                        </CollapsibleSection>
                    </div>

                    {/* World Config */}
                    <div className="grid-half">
                        <CollapsibleSection title={t("server_detail.headers.world_config")} icon={Globe} defaultOpen={true}>
                            <div className="form-column">
                                <div className="form-group">
                                    <label>World Name</label>
                                    <input
                                        type="text"
                                        value={configFormData.world_name || "default"}
                                        onChange={(e) => updateConfigValue("world_name", e.target.value)}
                                        className="input font-mono"
                                    />
                                </div>
                                <div className="form-group">
                                    <label>Game Mode (Default)</label>
                                    <Select
                                        options={[
                                            { label: "Adventure", value: "Adventure" },
                                            { label: "Creative", value: "Creative" },
                                        ]}
                                        value={configFormData.game_mode || "Adventure"}
                                        onChange={(v) => updateConfigValue("game_mode", v)}
                                    />
                                </div>
                                <div className="form-group">
                                    <label>{t("server_detail.config.generation")}</label>
                                    <Select
                                        options={[
                                            { label: "Hytale Default", value: "Hytale" },
                                            { label: "Flat World", value: "Flat" },
                                        ]}
                                        value={configFormData.world_gen_type || "Hytale"}
                                        onChange={(v) => updateConfigValue("world_gen_type", v)}
                                    />
                                </div>
                                <div className="form-group">
                                    <label>{t("server_detail.config.seed")}</label>
                                    <input
                                        type="text"
                                        value={configFormData.seed || ""}
                                        onChange={(e) => updateConfigValue("seed", e.target.value)}
                                        className="input font-mono"
                                        placeholder="Random"
                                    />
                                </div>
                                <div className="form-group">
                                    <div className="label-row">
                                        <label>{t("server_detail.config.view_distance")}</label>
                                        <span className="value-display">{configFormData.view_distance || 12} Chunks</span>
                                    </div>
                                    <RangeSlider
                                        min={4}
                                        max={32}
                                        value={configFormData.view_distance || 12}
                                        onChange={(v) => updateConfigValue("view_distance", v)}
                                    />
                                </div>
                                <div className="form-group">
                                    <label>{t("server_detail.config.max_players")}</label>
                                    <input
                                        type="number"
                                        value={configFormData.max_players || 100}
                                        onChange={(e) => updateConfigValue("max_players", parseInt(e.target.value))}
                                        className="input"
                                    />
                                </div>
                            </div>
                        </CollapsibleSection>
                    </div>

                    {/* Gameplay Toggles */}
                    <div className="grid-full">
                        <CollapsibleSection title={t("server_detail.config.gameplay_title")} icon={Check} defaultOpen={true}>
                            <div className="toggles-grid">
                                <Checkbox checked={configFormData.is_pvp_enabled !== false} onChange={(v: boolean) => updateConfigValue("is_pvp_enabled", v)} label={t("server_detail.config.pvp")} />
                                <Checkbox checked={configFormData.is_fall_damage_enabled !== false} onChange={(v: boolean) => updateConfigValue("is_fall_damage_enabled", v)} label={t("server_detail.config.fall_damage")} />
                                <Checkbox checked={configFormData.is_spawning_npc !== false} onChange={(v: boolean) => updateConfigValue("is_spawning_npc", v)} label={t("server_detail.config.npc_spawn")} />
                                <Checkbox checked={configFormData.is_game_time_paused !== true} onChange={(v: boolean) => updateConfigValue("is_game_time_paused", v)} label={t("server_detail.config.day_night_cycle")} />
                                <Checkbox checked={configFormData.is_saving_players !== false} onChange={(v: boolean) => updateConfigValue("is_saving_players", v)} label={t("server_detail.config.save_players")} />
                                <Checkbox checked={configFormData.is_saving_chunks !== false} onChange={(v: boolean) => updateConfigValue("is_saving_chunks", v)} label={t("server_detail.config.save_world")} />
                            </div>
                        </CollapsibleSection>
                    </div>

                    {/* Danger Zone */}
                    <div className="grid-full">
                        <CollapsibleSection
                            title={t("server_detail.config.danger_zone")}
                            icon={AlertTriangle}
                            defaultOpen={false}
                            badge={t("server_detail.config.sensitive_zone")}
                            className="danger-section"
                        >
                            <div className="danger-zone">
                                <p className="text-sm text-muted mb-4">
                                    {t("server_detail.config.danger_desc")}
                                </p>
                                <div className="danger-actions">
                                    <button
                                        type="button"
                                        onClick={onReinstall}
                                        className="btn btn--danger-outline"
                                    >
                                        <RotateCw size={16} /> {t("server_detail.config.reinstall")}
                                    </button>
                                    <button
                                        type="button"
                                        onClick={onDelete}
                                        className="btn btn--danger"
                                    >
                                        <Trash2 size={16} /> {t("server_detail.config.delete")}
                                    </button>
                                </div>
                            </div>
                        </CollapsibleSection>
                    </div>

                </div>
            </form>
        </div>
    );
}
