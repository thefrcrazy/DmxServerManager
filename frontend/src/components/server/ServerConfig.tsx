import React from "react";
import { Server as ServerIcon, Terminal, Cpu, Globe, Save, ChevronDown, Check } from "lucide-react";
import Checkbox from "../../components/Checkbox";
import RangeSlider from "../../components/RangeSlider";
import Select from "../../components/Select";

interface ServerConfigProps {
    configFormData: any;
    configSaving: boolean;
    configError: string;
    javaVersions: { path: string; version: string }[];
    updateConfigValue: (key: any, value: any) => void;
    toggleJvmArg: (arg: string) => void;
    handleSaveConfig: (e: React.FormEvent) => void;
    t: any;
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

const CollapsibleSection = ({
    title,
    icon: Icon,
    children,
    badge,
    defaultOpen = false,
}: any) => (
    <details className="config-section" open={defaultOpen}>
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
    t
}: ServerConfigProps) {
    return (
        <div className="config-wrapper">
            <form onSubmit={handleSaveConfig} className="config-form">

                {/* Header Action Bar */}
                <div className="config-action-bar">
                    <div className="action-info">
                        <div className="icon-circle">
                            <Save size={20} />
                        </div>
                        <div className="text-group">
                            <h3>Configuration du Serveur</h3>
                            <p>N'oubliez pas d'enregistrer vos modifications.</p>
                        </div>
                    </div>
                    <button
                        type="submit"
                        disabled={configSaving}
                        className="btn btn--primary"
                    >
                        {configSaving ? "Sauvegarde..." : "Enregistrer"}
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
                                    <label>Nom du serveur</label>
                                    <input
                                        type="text"
                                        value={configFormData.name || ""}
                                        onChange={(e) => updateConfigValue("name", e.target.value)}
                                        className="input"
                                        placeholder="Mon Serveur Hytale"
                                    />
                                    <p className="helper-text">Le nom affiché dans le manager.</p>
                                </div>
                                <div className="form-group">
                                    <label>Mode d'Authentification</label>
                                    <Select
                                        options={[
                                            { label: "Authenticated (Online Mode)", value: "authenticated" },
                                            { label: "Offline (Insecure)", value: "offline" },
                                        ]}
                                        value={configFormData.auth_mode || "authenticated"}
                                        onChange={(v) => updateConfigValue("auth_mode", v)}
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
                                        <label>RAM Minimale (-Xms)</label>
                                        <input
                                            type="text"
                                            value={configFormData.min_memory || ""}
                                            onChange={(e) => updateConfigValue("min_memory", e.target.value)}
                                            className="input font-mono"
                                            placeholder="ex: 1G"
                                        />
                                    </div>
                                    <div className="form-group">
                                        <label>RAM Maximale (-Xmx)</label>
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
                                        <label>Version Java</label>
                                        <Select
                                            options={[
                                                { label: "Défaut Système", value: "" },
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
                                        <label>Arguments JVM</label>
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
                                <label>Optimisations Recommandées</label>
                                <div className="suggestions-grid">
                                    {JVM_ARGS_SUGGESTIONS.map(({ arg, key }) => (
                                        <button
                                            key={key}
                                            type="button"
                                            onClick={() => toggleJvmArg(arg)}
                                            className={`suggestion-chip ${(configFormData.extra_args || "").includes(arg) ? "active" : ""}`}
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
                        <CollapsibleSection title={t("server_detail.headers.launch_args")} icon={Terminal}>
                            <div className="form-column">
                                <div className="form-group">
                                    <label>Adresse IP (--bind)</label>
                                    <input
                                        type="text"
                                        value={configFormData.bind_address || "0.0.0.0"}
                                        onChange={(e) => updateConfigValue("bind_address", e.target.value)}
                                        className="input font-mono"
                                    />
                                </div>
                                <div className="form-group">
                                    <label>Port (UDP)</label>
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
                                        onChange={(v) => updateConfigValue("allow_op", v)}
                                        label="Autoriser OP"
                                        description="Permet l'administration in-game"
                                    />
                                    <Checkbox
                                        checked={configFormData.disable_sentry || false}
                                        onChange={(v) => updateConfigValue("disable_sentry", v)}
                                        label="Désactiver Sentry"
                                        description="Pas de rapport d'erreur automatique"
                                    />
                                    <Checkbox
                                        checked={configFormData.accept_early_plugins || false}
                                        onChange={(v) => updateConfigValue("accept_early_plugins", v)}
                                        label="Early Plugins"
                                        description="Autoriser les plugins beta"
                                    />
                                </div>
                            </div>
                        </CollapsibleSection>
                    </div>

                    {/* World Config */}
                    <div className="grid-half">
                        <CollapsibleSection title={t("server_detail.headers.world_config")} icon={Globe}>
                            <div className="form-column">
                                <div className="form-group">
                                    <label>Génération</label>
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
                                    <label>Seed</label>
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
                                        <label>Distance de Vue</label>
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
                                    <label>Joueurs Max</label>
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
                        <CollapsibleSection title="Gameplay & Règles" icon={Check}>
                            <div className="toggles-grid">
                                <Checkbox checked={configFormData.is_pvp_enabled !== false} onChange={v => updateConfigValue("is_pvp_enabled", v)} label="PvP Actif" />
                                <Checkbox checked={configFormData.is_fall_damage_enabled !== false} onChange={v => updateConfigValue("is_fall_damage_enabled", v)} label="Dégâts de chute" />
                                <Checkbox checked={configFormData.is_spawning_npc !== false} onChange={v => updateConfigValue("is_spawning_npc", v)} label="Spawn de NPCs" />
                                <Checkbox checked={configFormData.is_game_time_paused !== true} onChange={v => updateConfigValue("is_game_time_paused", v)} label="Cycle Jour/Nuit" />
                                <Checkbox checked={configFormData.is_saving_players !== false} onChange={v => updateConfigValue("is_saving_players", v)} label="Save Players" />
                                <Checkbox checked={configFormData.is_saving_chunks !== false} onChange={v => updateConfigValue("is_saving_chunks", v)} label="Save World" />
                            </div>
                        </CollapsibleSection>
                    </div>

                </div>
            </form>
        </div>
    );
}
