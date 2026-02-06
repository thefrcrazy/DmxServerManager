import { useState, useEffect } from "react";
import { useNavigate } from "react-router-dom";
import {
    Plus, Server, FolderOpen, Upload, FolderArchive,
    Rocket, Play, Search, Cpu
} from "lucide-react";
import { useLanguage } from "../contexts/LanguageContext";
import { usePageTitle } from "../contexts/PageTitleContext";
import { Tooltip } from "@/components/ui";

type CreationMode = "normal" | "existing" | "zip";

interface ServerFormData {
    name: string;
    game_type: "hytale";
    executable_path: string;
    working_dir: string;

    bind_address: string;
    port: number;

    auth_mode: "authenticated" | "offline";
    allow_op: boolean;

    max_memory: string;
    min_memory: string;
    java_path: string;
    extra_args: string;
    nice_level: number;

    accept_early_plugins: boolean;

    backup_enabled: boolean;
    backup_dir: string;
    backup_frequency: number;

    auto_start: boolean;
    disable_sentry: boolean;

    seed: string;
    world_gen_type: string;
    view_distance: number;

    is_pvp_enabled: boolean;
    is_fall_damage_enabled: boolean;
    is_spawning_npc: boolean;
    is_game_time_paused: boolean;
    is_saving_players: boolean;

    is_ticking: boolean;
    is_block_ticking: boolean;
    is_all_npc_frozen: boolean;

    is_saving_chunks: boolean;
    is_unloading_chunks: boolean;

    is_spawn_markers_enabled: boolean;
    is_compass_updating: boolean;
    is_objective_markers_enabled: boolean;

    delete_on_universe_start: boolean;
    delete_on_remove: boolean;
}

export default function CreateServer() {
    const { t } = useLanguage();
    const navigate = useNavigate();
    const [creationMode, setCreationMode] = useState<CreationMode>("normal");
    const [formData, setFormData] = useState<ServerFormData>({
        // Section 1: Informations générales
        name: "",
        game_type: "hytale",
        executable_path: "HytaleServer.jar",
        working_dir: "",

        // Section 2: Configuration Réseau
        bind_address: "0.0.0.0",
        port: 5520,

        // Defaults hidden
        auth_mode: "authenticated",
        allow_op: false,
        max_memory: "4G",
        min_memory: "4G",
        java_path: "",
        extra_args: "",
        nice_level: 0,

        accept_early_plugins: false,
        backup_enabled: false,
        backup_dir: "",
        backup_frequency: 30,
        auto_start: false,
        disable_sentry: false,
        seed: "",
        world_gen_type: "Hytale",
        view_distance: 12,
        is_pvp_enabled: true,
        is_fall_damage_enabled: true,
        is_spawning_npc: true,
        is_game_time_paused: false,
        is_saving_players: true,
        is_ticking: true,
        is_block_ticking: true,
        is_all_npc_frozen: false,
        is_saving_chunks: true,
        is_unloading_chunks: true,
        is_spawn_markers_enabled: true,
        is_compass_updating: true,
        is_objective_markers_enabled: true,
        delete_on_universe_start: false,
        delete_on_remove: false,
    });
    const [zipFile, setZipFile] = useState<File | null>(null);
    const [isSubmitting, setIsSubmitting] = useState(false);
    const [error, setError] = useState("");
    const [isDocker, setIsDocker] = useState(false);
    const [defaultServersDir, setDefaultServersDir] = useState("");
    const [isDetectingJava, setIsDetectingJava] = useState(false);

    const { setPageTitle } = usePageTitle();
    useEffect(() => {
        setPageTitle(t("servers.create_new"), t("servers.create_subtitle"), { to: "/servers" });

        // Fetch settings specifically to check for Docker env and default dir
        fetch("/api/v1/settings", {
            headers: { Authorization: `Bearer ${localStorage.getItem("token")}` }
        })
            .then(res => res.json())
            .then(data => {
                setIsDocker(data.is_docker);
                setDefaultServersDir(data.servers_dir);
                // Initialize working dir if empty
                setFormData(prev => ({
                    ...prev,
                    working_dir: prev.working_dir || data.servers_dir
                }));
            })
            .catch(console.error);
    }, [setPageTitle, t]);

    // Auto-update working_dir in Docker mode when name changes
    useEffect(() => {
        if (isDocker && formData.name) {
            setFormData(prev => ({
                ...prev,
                working_dir: defaultServersDir
            }));
        }
    }, [formData.name, isDocker, defaultServersDir]);

    const creationModes = [
        { id: "normal" as CreationMode, label: t("servers.create_new"), icon: Plus, description: t("servers.create_mode_normal_desc") },
        { id: "existing" as CreationMode, label: t("servers.import_existing"), icon: FolderOpen, description: t("servers.create_mode_existing_desc") },
        { id: "zip" as CreationMode, label: t("servers.import_zip"), icon: FolderArchive, description: t("servers.create_mode_zip_desc") },
    ];

    const detectJava = async () => {
        setIsDetectingJava(true);
        try {
            const res = await fetch("/api/v1/system/java-versions", {
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` }
            });
            if (res.ok) {
                const versions = await res.json();
                if (versions.length > 0) {
                    // Pick the first one or logic to pick best
                    updateFormData("java_path", versions[0].path);
                }
            }
        } catch (e) {
            console.error("Failed to detect java", e);
        } finally {
            setIsDetectingJava(false);
        }
    };

    const handleSubmit = async (e: React.FormEvent) => {
        e.preventDefault();
        setIsSubmitting(true);
        setError("");

        try {
            if (creationMode === "zip" && zipFile) {
                const formDataUpload = new FormData();
                formDataUpload.append("file", zipFile);
                formDataUpload.append("name", formData.name);
                formDataUpload.append("min_memory", formData.min_memory);
                formDataUpload.append("max_memory", formData.max_memory);
                formDataUpload.append("auto_start", String(formData.auto_start));
                if (formData.java_path) formDataUpload.append("java_path", formData.java_path);
                if (formData.extra_args) formDataUpload.append("extra_args", formData.extra_args);
                formDataUpload.append("nice_level", String(formData.nice_level));

                const response = await fetch("/api/v1/servers/import-zip", {
                    method: "POST",
                    headers: {
                        Authorization: `Bearer ${localStorage.getItem("token")}`,
                    },
                    body: formDataUpload,
                });

                if (response.ok) {
                    navigate("/servers");
                } else {
                    const data = await response.json();
                    setError(data.error || t("servers.import_error"));
                }
            } else {
                const response = await fetch("/api/v1/servers", {
                    method: "POST",
                    headers: {
                        "Content-Type": "application/json",
                        Authorization: `Bearer ${localStorage.getItem("token")}`,
                    },
                    body: JSON.stringify({
                        ...formData,
                        game_type: formData.game_type,
                        java_path: formData.java_path || null,
                        extra_args: formData.extra_args || null,
                        // assets_path: fixed to Assets.zip by backend
                        backup_dir: formData.backup_dir || null,
                        seed: formData.seed || null,
                        import_existing: creationMode === "existing",
                    }),
                });

                if (response.ok) {
                    const data = await response.json();
                    // Redirect to server detail to see installation logs
                    navigate(`/servers/${data.id}`);
                } else {
                    const data = await response.json();
                    setError(data.error || t("servers.create_error"));
                }
            }
        } catch (err) {
            setError(t("panel_settings.connection_error"));
            console.error("Erreur:", err);
        } finally {
            setIsSubmitting(false);
        }
    };

    const handleZipChange = (e: React.ChangeEvent<HTMLInputElement>) => {
        const file = e.target.files?.[0];
        if (file && file.name.endsWith(".zip")) {
            setZipFile(file);
            setError("");
        } else {
            setError(t("servers.zip_error"));
        }
    };

    const updateFormData = <K extends keyof ServerFormData>(key: K, value: ServerFormData[K]) => {
        setFormData(prev => ({ ...prev, [key]: value }));
    };

    return (
        <div className="create-server-page">

            {/* Creation Mode Tabs */}
            <div className="creation-mode-tabs">
                {creationModes.map((mode) => (
                    <button
                        key={mode.id}
                        type="button"
                        onClick={() => setCreationMode(mode.id)}
                        className={`creation-mode-btn ${creationMode === mode.id ? "creation-mode-btn--active" : ""}`}
                    >
                        <mode.icon size={24} />
                        <span>{mode.label}</span>
                    </button>
                ))}
            </div>

            {/* Form */}
            <form onSubmit={handleSubmit}>
                <div className="card server-config-card">
                    <h3 className="server-config-title">
                        <Server size={20} />
                        {t("servers.config_title")}
                    </h3>

                    <div className="server-config-grid">

                        {/* Server Name */}
                        <div className="form-group">
                            <label>{t("servers.server_name")}</label>
                            <input
                                type="text"
                                value={formData.name}
                                onChange={(e) => updateFormData("name", e.target.value)}
                                placeholder="Mon Serveur"
                                required
                                className="input"
                            />
                        </div>

                        {/* Game Type */}
                        <div className="form-group">
                            <label>{t("servers.game_type")}</label>
                            <select
                                value={formData.game_type}
                                onChange={(e) => updateFormData("game_type", e.target.value as any)}
                                className="input"
                            >
                                <option value="hytale">Hytale</option>
                            </select>
                        </div>



                        {/* ZIP Upload or Directory */}
                        {creationMode === "zip" ? (
                            <div className="form-group">
                                <label className="form-label-icon">
                                    <Upload size={14} />
                                    {t("servers.zip_file")}
                                </label>
                                <div className={`zip-upload-zone ${zipFile ? "zip-upload-zone--active" : ""}`}>
                                    <input
                                        type="file"
                                        accept=".zip"
                                        onChange={handleZipChange}
                                        id="zip-upload"
                                        className="hidden-input"
                                    />
                                    <label htmlFor="zip-upload" className="zip-upload-content">
                                        {zipFile ? (
                                            <>
                                                <FolderArchive size={32} className="zip-upload-file-icon" />
                                                <p className="zip-upload-file-name">{zipFile.name}</p>
                                                <p className="helper-text">
                                                    {(zipFile.size / 1024 / 1024).toFixed(2)} Mo
                                                </p>
                                            </>
                                        ) : (
                                            <>
                                                <Upload size={32} className="zip-upload-icon" />
                                                <p className="zip-upload-text">{t("servers.click_to_select")}</p>
                                                <p className="helper-text">
                                                    {t("servers.zip_helper")}
                                                </p>
                                            </>
                                        )}
                                    </label>
                                </div>
                            </div>
                        ) : (
                            /* Working Directory */
                            <div className="form-group">
                                <label className="form-label-icon">
                                    <FolderOpen size={14} />
                                    {creationMode === "existing" ? t("servers.existing_dir") : t("servers.working_dir")}
                                </label>
                                <input
                                    type="text"
                                    value={formData.working_dir}
                                    onChange={(e) => updateFormData("working_dir", e.target.value)}
                                    placeholder="/home/hytale/servers/mon-serveur"
                                    required
                                    className={`input font-mono ${isDocker ? "server-config-item--disabled" : ""}`}
                                    readOnly={isDocker}
                                />
                                <p className="helper-text helper-text--block">
                                    {isDocker
                                        ? t("servers.docker_hint")
                                        : creationMode === "existing"
                                            ? t("servers.existing_hint")
                                            : t("servers.new_hint")}
                                </p>
                            </div>
                        )}

                        <div className="server-config-row">
                            {/* RAM Min */}
                            <div className="form-group">
                                <label>{t("servers.ram_min")}</label>
                                <input
                                    type="text"
                                    value={formData.min_memory}
                                    onChange={(e) => updateFormData("min_memory", e.target.value)}
                                    placeholder="4G"
                                    className="input"
                                />
                            </div>

                            {/* RAM Max */}
                            <div className="form-group">
                                <label>{t("servers.ram_max")}</label>
                                <input
                                    type="text"
                                    value={formData.max_memory}
                                    onChange={(e) => updateFormData("max_memory", e.target.value)}
                                    placeholder="4G"
                                    className="input"
                                />
                            </div>
                        </div>

                        {/* Java Path with Auto-Detect */}
                        <div className="form-group">
                            <label className="form-label-icon">
                                <Search size={14} />
                                {t("servers.java_path") || "Chemin Java"}
                            </label>
                            <div className="input-group">
                                <input
                                    type="text"
                                    value={formData.java_path}
                                    onChange={(e) => updateFormData("java_path", e.target.value)}
                                    placeholder="java (ou chemin complet)"
                                    className="input"
                                />
                                <Tooltip content="Détecter Java" position="top">
                                    <button 
                                        type="button" 
                                        className="btn btn--secondary btn--icon"
                                        onClick={detectJava}
                                        disabled={isDetectingJava}
                                    >
                                        <Search size={16} className={isDetectingJava ? "spin" : ""} />
                                    </button>
                                </Tooltip>
                            </div>
                        </div>

                        {/* CPU / Nice */}
                        <div className="form-group">
                            <label className="form-label-icon">
                                <Cpu size={14} />
                                {t("servers.cpu_priority") || "Priorité CPU (Nice)"}
                            </label>
                            <div className="range-wrapper">
                                <input
                                    type="range"
                                    min="-20"
                                    max="19"
                                    step="1"
                                    value={formData.nice_level}
                                    onChange={(e) => updateFormData("nice_level", parseInt(e.target.value))}
                                    className="range-input"
                                />
                                <div className="range-value">
                                    <span>Haute (-20)</span>
                                    <span className="font-mono font-bold text-primary">{formData.nice_level}</span>
                                    <span>Basse (19)</span>
                                </div>
                            </div>
                            <p className="helper-text">
                                {t("servers.cpu_priority_hint") || "Valeur négative = priorité plus haute (Linux/Mac). 0 = Défaut."}
                            </p>
                        </div>

                        {/* Port UDP */}
                        <div className="form-group">
                            <label>{t("servers.udp_port")}</label>
                            <input
                                type="number"
                                value={formData.port}
                                onChange={(e) => updateFormData("port", parseInt(e.target.value) || 5520)}
                                placeholder="5520"
                                className="input"
                            />
                        </div>

                        {/* Hidden Defaults Confirmation */}
                        <div className="advanced-defaults">
                            <div className="advanced-defaults__header">
                                <Rocket size={14} />
                                <span>{t("servers.advanced_defaults")}</span>
                            </div>
                            <ul>
                                <li>{t("servers.default_jvm")}</li>
                                <li>{t("servers.default_auth")}</li>
                                <li>{t("servers.default_backups")}</li>
                            </ul>
                        </div>

                    </div>

                    {error && (
                        <div className="error-banner">
                            {error}
                        </div>
                    )}

                    <div className="form-footer">
                        <button
                            type="button"
                            className="btn btn--secondary"
                            onClick={() => navigate("/servers")}
                        >
                            {t("common.cancel")}
                        </button>
                        <button
                            type="submit"
                            className="btn btn--primary btn--lg"
                            disabled={isSubmitting}
                        >
                            {isSubmitting ? (
                                <div className="spinner-sm" />
                            ) : (
                                <>
                                    {creationMode === "zip" ? <Upload size={18} /> : <Play size={18} />}
                                    {creationMode === "existing" ? t("servers.import_btn") : t("servers.create_btn")}
                                </>
                            )}
                        </button>
                    </div>
                </div>
            </form>
            <style>{`
                .input-group {
                    display: flex;
                    gap: 8px;
                }
                .input-group .input {
                    flex: 1;
                }
                .range-wrapper {
                    display: flex;
                    flex-direction: column;
                    gap: 8px;
                }
                .range-input {
                    width: 100%;
                    cursor: pointer;
                }
                .range-value {
                    display: flex;
                    justify-content: space-between;
                    font-size: 0.8rem;
                    color: var(--text-secondary);
                }
            `}</style>
        </div>
    );
}