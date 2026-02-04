import { useState, useEffect } from "react";
import { useParams, useNavigate, Link } from "react-router-dom";
import {
    User as UserIcon,
    Key,
    Shield,
    Globe,
    Palette,
    Server,
    Clock,
    Check,
    Save
} from "lucide-react";
import Select from "@/components/ui/Select";
import Checkbox from "@/components/ui/Checkbox";
import { useLanguage } from "../contexts/LanguageContext";
import { usePageTitle } from "../contexts/PageTitleContext";
import { PRESET_COLORS, LANGUAGES } from "@/constants/theme";

interface User {
    id: string;
    username: string;
    role: "admin" | "user";
    is_active: boolean;
    language: string;
    accent_color: string;
    created_at: string;
    updated_at: string;
    last_login: string | null;
    last_ip: string | null;
    allocated_servers: string[];
}

interface ServerInfo {
    id: string;
    name: string;
}

export default function EditUser() {
    const { id } = useParams<{ id: string }>();
    const navigate = useNavigate();
    const { t } = useLanguage();

    const isCreating = id === "new";

    const [user, setUser] = useState<User | null>(null);
    const [servers, setServers] = useState<ServerInfo[]>([]);
    const [isLoading, setIsLoading] = useState(true);
    const [isSaving, setIsSaving] = useState(false);
    const [error, setError] = useState("");

    const [formData, setFormData] = useState({
        username: "",
        password: "",
        role: "user" as "admin" | "user",
        is_active: true,
        language: "fr",
        accent_color: "#3A82F6",
        allocated_servers: [] as string[]
    });

    useEffect(() => {
        fetchData();
    }, [id]);

    const { setPageTitle } = usePageTitle();
    useEffect(() => {
        const title = isCreating ? t("users.create_user_title") : t("users.edit_user_title");
        const subtitle = isCreating ? t("users.create_user_subtitle") : t("users.edit_user_subtitle").replace("{{username}}", user?.username || "");
        setPageTitle(title, subtitle, { to: "/panel-settings?tab=users" });
    }, [setPageTitle, t, isCreating, user]);

    const fetchData = async () => {
        try {
            const serversRes = await fetch("/api/v1/servers", {
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });
            if (serversRes.ok) {
                setServers(await serversRes.json());
            }

            if (!isCreating && id) {
                const userRes = await fetch(`/api/v1/users/${id}`, {
                    headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
                });
                if (userRes.ok) {
                    const userData = await userRes.json();
                    setUser(userData);
                    setFormData({
                        username: userData.username,
                        password: "",
                        role: userData.role,
                        is_active: userData.is_active,
                        language: userData.language,
                        accent_color: userData.accent_color,
                        allocated_servers: userData.allocated_servers || []
                    });
                } else {
                    navigate("/panel-settings?tab=users");
                }
            }
        } catch (error) {
            console.error("Erreur:", error);
        } finally {
            setIsLoading(false);
        }
    };

    const handleSubmit = async (e: React.FormEvent) => {
        e.preventDefault();
        setError("");
        setIsSaving(true);

        try {
            const url = isCreating ? "/api/v1/users" : `/api/v1/users/${id}`;
            const method = isCreating ? "POST" : "PUT";

            const body: Record<string, unknown> = {
                username: formData.username,
                role: formData.role,
                is_active: formData.is_active,
                language: formData.language,
                accent_color: formData.accent_color,
                allocated_servers: formData.allocated_servers
            };

            if (formData.password) {
                body.password = formData.password;
            }

            const response = await fetch(url, {
                method,
                headers: {
                    "Content-Type": "application/json",
                    Authorization: `Bearer ${localStorage.getItem("token")}`,
                },
                body: JSON.stringify(body),
            });

            if (response.ok) {
                navigate("/panel-settings?tab=users");
            } else {
                const data = await response.json();
                setError(data.error || t("common.error"));
            }
        } catch (err) {
            setError(t("panel_settings.connection_error"));
            console.error("Erreur:", err);
        } finally {
            setIsSaving(false);
        }
    };

    const toggleServerAllocation = (serverId: string) => {
        setFormData(prev => ({
            ...prev,
            allocated_servers: prev.allocated_servers.includes(serverId)
                ? prev.allocated_servers.filter(id => id !== serverId)
                : [...prev.allocated_servers, serverId]
        }));
    };

    const formatDate = (dateStr: string | null) => {
        if (!dateStr) return "—";
        return new Date(dateStr).toLocaleDateString(formData.language, {
            day: "2-digit",
            month: "short",
            year: "numeric",
            hour: "2-digit",
            minute: "2-digit"
        });
    };

    if (isLoading) return <div>{t("common.loading")}</div>;

    return (
        <div className="edit-user-page">
            {/* Form */}
            <form onSubmit={handleSubmit}>
                <div className="content-container">

                    {/* Left Column - Account Info */}
                    <div className="card form-section">
                        <h3 className="form-section-title">
                            <UserIcon size={18} />
                            {t("users.account_info")}
                        </h3>

                        <div className="form-column">
                            {/* Username */}
                            <div className="form-group">
                                <label>{t("users.username")}</label>
                                <input
                                    type="text"
                                    value={formData.username}
                                    onChange={(e) => setFormData({ ...formData, username: e.target.value })}
                                    placeholder="nom_utilisateur"
                                    required
                                    className="input"
                                />
                            </div>

                            {/* Password */}
                            <div className="form-group">
                                <label className="form-label-icon">
                                    <Key size={14} className="text-accent" />
                                    {isCreating ? t("auth.password") : t("user_settings.new_password")}
                                </label>
                                <input
                                    type="password"
                                    value={formData.password}
                                    onChange={(e) => setFormData({ ...formData, password: e.target.value })}
                                    placeholder={isCreating ? "" : t("users.password_placeholder")}
                                    required={isCreating}
                                    className="input"
                                />
                                {!isCreating && (
                                    <p className="helper-text">
                                        {t("users.password_leave_empty")}
                                    </p>
                                )}
                            </div>

                            {/* Role & Language */}
                            <div className="form-grid-2">
                                <div className="form-group">
                                    <label className="form-label-icon">
                                        <Shield size={14} className="text-accent" />
                                        {t("users.role")}
                                    </label>
                                    <Select
                                        options={[
                                            { label: t("user_settings.role_user"), value: "user", icon: <UserIcon size={14} /> },
                                            { label: t("user_settings.role_admin"), value: "admin", icon: <Shield size={14} /> }
                                        ]}
                                        value={formData.role}
                                        onChange={(value) => setFormData({ ...formData, role: value as "admin" | "user" })}
                                    />
                                </div>
                                <div className="form-group">
                                    <label className="form-label-icon">
                                        <Globe size={14} className="text-accent" />
                                        {t("settings.language")}
                                    </label>
                                    <Select
                                        options={LANGUAGES.map(lang => ({
                                            label: lang.name,
                                            value: lang.code
                                        }))}
                                        value={formData.language}
                                        onChange={(value) => setFormData({ ...formData, language: value })}
                                    />
                                </div>
                            </div>

                            {/* Active Status */}
                            <Checkbox
                                checked={formData.is_active}
                                onChange={(v) => setFormData({ ...formData, is_active: v })}
                                label={t("users.active_account")}
                                description={t("users.active_desc")}
                                className="full-width-checkbox"
                            />
                        </div>
                    </div>

                    {/* Right Column - Personalization */}
                    <div className="card form-section">
                        <h3 className="form-section-title">
                            <Palette size={18} />
                            {t("users.personalization")}
                        </h3>

                        <div className="form-column">
                            {/* Accent Color */}
                            <div className="form-group">
                                <label>{t("user_settings.accent_color")}</label>
                                <div className="color-picker">
                                    {PRESET_COLORS.map((color) => (
                                        <button
                                            key={color}
                                            type="button"
                                            onClick={() => setFormData({ ...formData, accent_color: color })}
                                            className={`color-picker__swatch ${formData.accent_color.toLowerCase() === color.toLowerCase() ? "color-picker__swatch--active" : ""}`}
                                            style={{
                                                background: color,
                                                boxShadow: formData.accent_color.toLowerCase() === color.toLowerCase()
                                                    ? `0 0 15px ${color}66`
                                                    : "none"
                                            }}
                                        >
                                            {formData.accent_color.toLowerCase() === color.toLowerCase() && (
                                                <Check size={16} color="white" strokeWidth={3} />
                                            )}
                                        </button>
                                    ))}
                                    <div className="color-picker__custom">
                                        <input
                                            type="color"
                                            value={formData.accent_color}
                                            onChange={(e) => setFormData({ ...formData, accent_color: e.target.value })}
                                        />
                                    </div>
                                </div>
                            </div>

                            {/* Server Allocation */}
                            <div className="form-group">
                                <label className="form-label-icon">
                                    <Server size={14} className="text-accent" />
                                    {t("users.server_allocation")}
                                </label>
                                {servers.length === 0 ? (
                                    <p className="helper-text">
                                        {t("users.no_servers_avail")}
                                    </p>
                                ) : (
                                    <div className="server-allocation">
                                        {servers.map(server => (
                                            <button
                                                key={server.id}
                                                type="button"
                                                className={`server-allocation__item ${formData.allocated_servers.includes(server.id) ? "server-allocation__item--active" : ""}`}
                                                onClick={() => toggleServerAllocation(server.id)}
                                            >
                                                {formData.allocated_servers.includes(server.id) && (
                                                    <Check size={14} />
                                                )}
                                                {server.name}
                                            </button>
                                        ))}
                                    </div>
                                )}
                            </div>
                        </div>
                    </div>

                    {/* Full Width - User Info (only when editing) */}
                    {!isCreating && user && (
                        <div className="card form-section full-width">
                            <h3 className="form-section-title">
                                <Clock size={18} />
                                {t("users.system_info")}
                            </h3>

                            <div className="system-info-grid">
                                <div className="system-info-card">
                                    <p className="system-info-card__label">
                                        {t("users.created_at")}
                                    </p>
                                    <p className="system-info-card__value">
                                        {formatDate(user.created_at)}
                                    </p>
                                </div>
                                <div className="system-info-card">
                                    <p className="system-info-card__label">
                                        {t("users.updated_at")}
                                    </p>
                                    <p className="system-info-card__value">
                                        {formatDate(user.updated_at)}
                                    </p>
                                </div>
                                <div className="system-info-card">
                                    <p className="system-info-card__label">
                                        {t("users.last_login")}
                                    </p>
                                    <p className="system-info-card__value">
                                        {formatDate(user.last_login)}
                                    </p>
                                </div>
                                <div className="system-info-card">
                                    <p className="system-info-card__label">
                                        {t("users.last_ip")}
                                    </p>
                                    <p className="system-info-card__value system-info-card__value--mono">
                                        {user.last_ip || "—"}
                                    </p>
                                </div>
                            </div>
                        </div>
                    )}
                </div>

                {/* Error Message */}
                {error && (
                    <div className="alert alert--error">
                        {error}
                    </div>
                )}

                {/* Actions */}
                <div className="action-footer">
                    <Link to="/panel-settings?tab=users" className="btn btn--secondary btn-cancel">
                        {t("common.cancel")}
                    </Link>
                    <button
                        type="submit"
                        className="btn btn--primary btn-save"
                        disabled={isSaving || !formData.username}
                    >
                        {isSaving ? (
                            t("common.loading")
                        ) : (
                            <>
                                <Save size={18} />
                                {isCreating ? t("users.create") : t("users.save")}
                            </>
                        )}
                    </button>
                </div>
            </form>
        </div>
    );
}
