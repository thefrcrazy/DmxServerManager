import { useState, useEffect } from "react";
import { Link, useSearchParams } from "react-router-dom";
import {
    Save, FolderOpen, AlertTriangle, Palette, Image, FolderSearch, Upload,
    Users, Shield, Plus, Edit2, Trash2, ShieldOff, User as UserIcon
} from "lucide-react";
import { DirectoryPicker, LoadingScreen } from "@/components/shared";
import { Table, Tabs, ColorPicker, Tooltip } from "@/components/ui";
import RoleManagement from "@/components/features/settings/RoleManagement";
import { useLanguage } from "@/contexts/LanguageContext";
import { usePageTitle } from "@/contexts/PageTitleContext";
import { useToast } from "@/contexts/ToastContext";
import { useDialog } from "@/contexts/DialogContext";

interface PanelInfo {
    version: string;
    servers_dir: string;
    backups_dir: string;
    database_path: string;
    is_docker: boolean;
}

interface LoginCustomization {
    default_color: string;
    background_url: string;
}

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

interface Role {
    id: string;
    name: string;
    permissions: string[];
    color: string;
}

type ActiveTab = "general" | "users" | "roles";

export default function PanelSettings() {
    const { t, language } = useLanguage();
    const { error: showError } = useToast();
    const { confirm } = useDialog();
    const [searchParams, setSearchParams] = useSearchParams();
    const tabParam = searchParams.get("tab") as ActiveTab | null;
    const [activeTab, setActiveTab] = useState<ActiveTab>(tabParam || "general");

    // Sync tab with URL
    useEffect(() => {
        const urlTab = searchParams.get("tab") as ActiveTab | null;
        if (urlTab && ["general", "users", "roles"].includes(urlTab)) {
            setActiveTab(urlTab);
        }
    }, [searchParams]);

    // Update URL when tab changes
    const handleTabChange = (tab: ActiveTab) => {
        setActiveTab(tab);
        setSearchParams({ tab });
    };

    // General settings state
    const [webhookUrl, setWebhookUrl] = useState("");
    const [panelInfo, setPanelInfo] = useState<PanelInfo>({
        version: "0.1.0",
        servers_dir: "./servers",
        backups_dir: "./backups",
        database_path: "database.db",
        is_docker: false
    });
    const [loginCustomization, setLoginCustomization] = useState<LoginCustomization>({
        default_color: "#3A82F6",
        background_url: ""
    });
    const [serversDir, setServersDir] = useState("");
    const [backupsDir, setBackupsDir] = useState("");
    const [isSaving, setIsSaving] = useState(false);
    const [saveSuccess, setSaveSuccess] = useState(false);
    const [saveMessage, setSaveMessage] = useState(t("panel_settings.save_message"));
    const [isLoading, setIsLoading] = useState(true);
    const [showServersDirPicker, setShowServersDirPicker] = useState(false);
    const [showBackupsDirPicker, setShowBackupsDirPicker] = useState(false);
    const [isTestingWebhook, setIsTestingWebhook] = useState(false);
    const [webhookTestResult, setWebhookTestResult] = useState<{ success: boolean; message: string } | null>(null);
    const [isUploadingImage, setIsUploadingImage] = useState(false);

    // Users state
    const [users, setUsers] = useState<User[]>([]);
    const [searchQuery, setSearchQuery] = useState("");

    // Roles state (placeholder for future)
    const [roles] = useState<Role[]>([
        { id: "1", name: t("user_settings.role_admin"), permissions: ["all"], color: "#FF591E" },
        { id: "2", name: t("user_settings.role_user"), permissions: ["view", "manage_own_servers"], color: "#3A82F6" },
    ]);

    useEffect(() => {
        fetchSettings();
        fetchUsers();
    }, []);

    const { setPageTitle } = usePageTitle();
    useEffect(() => {
        setPageTitle(t("panel_settings.title"), t("panel_settings.subtitle"));
    }, [setPageTitle, t]);

    const handleTestWebhook = async () => {
        setIsTestingWebhook(true);
        setWebhookTestResult(null);

        try {
            const response = await fetch("/api/v1/webhook/test", {
                method: "POST",
                headers: {
                    "Content-Type": "application/json",
                    "Authorization": `Bearer ${localStorage.getItem("token")}`,
                },
                body: JSON.stringify({ webhook_url: webhookUrl }),
            });

            if (response.ok) {
                setWebhookTestResult({ success: true, message: t("panel_settings.webhook_success") });
            } else {
                const data = await response.json();
                setWebhookTestResult({ success: false, message: data.error || t("common.error") });
            }
        } catch (error) {
            setWebhookTestResult({ success: false, message: t("common.error") });
        } finally {
            setIsTestingWebhook(false);
        }
    };

    const handleImageUpload = async (e: React.ChangeEvent<HTMLInputElement>) => {
        const file = e.target.files?.[0];
        if (!file) return;


        if (!file.type.startsWith("image/")) {
            showError(t("panel_settings.invalid_image"));
            return;
        }

        if (file.size > 5 * 1024 * 1024) {
            showError(t("panel_settings.image_size_error"));
            return;
        }

        setIsUploadingImage(true);
        try {
            const formData = new FormData();
            formData.append("file", file);

            const response = await fetch("/api/v1/upload/image", {
                method: "POST",
                headers: {
                    "Authorization": `Bearer ${localStorage.getItem("token")}`,
                },
                body: formData,
            });

            if (response.ok) {
                const data = await response.json();
                setLoginCustomization(prev => ({ ...prev, background_url: data.url }));
            } else {
                const errorData = await response.json();
                showError(errorData.error || t("panel_settings.upload_error"));
            }
        } catch (error) {
            showError(t("panel_settings.connection_error"));
        } finally {
            setIsUploadingImage(false);
        }
    };

    const fetchSettings = async () => {
        try {
            const response = await fetch("/api/v1/settings", {
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });
            if (response.ok) {
                const data = await response.json();
                setPanelInfo({
                    version: data.version || "0.1.0",
                    servers_dir: data.servers_dir || "./servers",
                    backups_dir: data.backups_dir || "./backups",
                    database_path: data.database_path || "database.db",
                    is_docker: data.is_docker || false
                });
                setServersDir(data.servers_dir || "./data/servers");
                setBackupsDir(data.backups_dir || "./data/backups");
                setWebhookUrl(data.webhook_url || "");
                setLoginCustomization({
                    default_color: data.login_default_color || "#3A82F6",
                    background_url: data.login_background_url || ""
                });
            }
        } catch (error) {
            console.error("Erreur:", error);
        } finally {
            setIsLoading(false);
        }
    };

    const fetchUsers = async () => {
        try {
            const response = await fetch("/api/v1/users", {
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });
            if (response.ok) {
                setUsers(await response.json());
            }
        } catch (error) {
            console.error("Erreur:", error);
        }
    };

    const handleSave = async () => {
        setIsSaving(true);
        setSaveSuccess(false);

        try {
            const response = await fetch("/api/v1/settings", {
                method: "PUT",
                headers: {
                    "Content-Type": "application/json",
                    Authorization: `Bearer ${localStorage.getItem("token")}`,
                },
                body: JSON.stringify({
                    webhook_url: webhookUrl,
                    servers_dir: panelInfo.is_docker ? undefined : serversDir,
                    backups_dir: panelInfo.is_docker ? undefined : backupsDir,
                    login_default_color: loginCustomization.default_color,
                    login_background_url: loginCustomization.background_url
                }),
            });

            if (response.ok) {
                setSaveMessage(t("panel_settings.save_success"));
                setSaveSuccess(true);
                fetchSettings();
                setTimeout(() => setSaveSuccess(false), 5000);
            }
        } catch (error) {
            console.error("Erreur:", error);
        } finally {
            setIsSaving(false);
        }
    };

    const handleDeleteUser = async (user: User) => {
        if (!await confirm(t("common.delete") + ` "${user.username}" ?`, { isDestructive: true })) return;
        try {
            const response = await fetch(`/api/v1/users/${user.id}`, {
                method: "DELETE",
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });
            if (response.ok) fetchUsers();
        } catch (error) {
            console.error("Erreur:", error);
        }
    };

    const handleToggleUserActive = async (user: User) => {
        try {
            await fetch(`/api/v1/users/${user.id}`, {
                method: "PUT",
                headers: {
                    "Content-Type": "application/json",
                    Authorization: `Bearer ${localStorage.getItem("token")}`,
                },
                body: JSON.stringify({ is_active: !user.is_active }),
            });
            fetchUsers();
        } catch (error) {
            console.error("Erreur:", error);
        }
    };

    const formatDate = (dateStr: string | null) => {
        if (!dateStr) return t("common.never");
        try {
            return new Date(dateStr).toLocaleDateString(language === "fr" ? "fr-FR" : "en-US", {
                day: "2-digit", month: "2-digit", year: "numeric", hour: "2-digit", minute: "2-digit"
            });
        } catch (e) {
            return dateStr;
        }
    };

    const filteredUsers = users.filter(u => u.username.toLowerCase().includes(searchQuery.toLowerCase()));

    if (isLoading) {
        return <LoadingScreen />;
    }

    const tabs = [
        { id: "general", label: t("panel_settings.tabs.general"), icon: <FolderOpen size={18} /> },
        { id: "users", label: t("panel_settings.tabs.users"), icon: <Users size={18} /> },
        { id: "roles", label: t("panel_settings.tabs.roles"), icon: <Shield size={18} /> },
    ];

    return (
        <div className="settings-page">

            {/* Tabs */}
            <Tabs
                tabs={tabs}
                activeTab={activeTab}
                onTabChange={handleTabChange}
                className="tabs-nav"
            />

            {/* General Tab */}
            {activeTab === "general" && (
                <div className="settings-grid">
                    {/* Discord Notifications */}
                    <div className="card">
                        <h3 className="settings-section__title">
                            <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor">
                                <path d="M20.317 4.37a19.791 19.791 0 0 0-4.885-1.515a.074.074 0 0 0-.079.037c-.21.375-.444.864-.608 1.25a18.27 18.27 0 0 0-5.487 0a12.64 12.64 0 0 0-.617-1.25a.077.077 0 0 0-.079-.037A19.736 19.736 0 0 0 3.677 4.37a.07.07 0 0 0-.032.027C.533 9.046-.32 13.58.099 18.057a.082.082 0 0 0 .031.057a19.9 19.9 0 0 0 5.993 3.03a.078.078 0 0 0 .084-.028a14.09 14.09 0 0 0 1.226-1.994a.076.076 0 0 0-.041-.106a13.107 13.107 0 0 1-1.872-.892a.077.077 0 0 1-.008-.128a10.2 10.2 0 0 0 .372-.292a.074.074 0 0 1 .077-.01c3.928 1.793 8.18 1.793 12.062 0a.074.074 0 0 1 .078.01c.12.098.246.198.373.292a.077.077 0 0 1-.006.127a12.299 12.299 0 0 1-1.873.892a.077.077 0 0 0-.041.107c.36.698.772 1.362 1.225 1.993a.076.076 0 0 0 .084.028a19.839 19.839 0 0 0 6.002-3.03a.077.077 0 0 0 .032-.054c.5-5.177-.838-9.674-3.549-13.66a.061.061 0 0 0-.031-.03z" />
                            </svg>
                            {t("panel_settings.discord_title")}
                        </h3>

                        <p className="form-hint mb-4">
                            {t("panel_settings.discord_hint")}
                        </p>

                        <div className="form-group">
                            <label className="form-label">{t("panel_settings.webhook_url")}</label>
                            <input
                                type="text"
                                placeholder="https://discord.com/api/webhooks/..."
                                className="form-input"
                                value={webhookUrl}
                                onChange={(e) => setWebhookUrl(e.target.value)}
                            />
                            <p className="form-hint">
                                {t("panel_settings.webhook_hint")}
                            </p>
                        </div>

                        <button
                            type="button"
                            className="btn btn--secondary"
                            onClick={handleTestWebhook}
                            disabled={!webhookUrl || isTestingWebhook}
                        >
                            {isTestingWebhook ? t("panel_settings.test_success") : t("panel_settings.test_webhook")}
                        </button>
                        {webhookTestResult && (
                            <p className={`form-hint mt-2 ${webhookTestResult.success ? "text-success" : "text-danger"}`}>
                                {webhookTestResult.message}
                            </p>
                        )}
                    </div>

                    {/* Paths */}
                    <div className="card">
                        <h3 className="settings-section__title">
                            <FolderOpen size={20} />
                            {t("panel_settings.general_title")}
                        </h3>

                        {panelInfo.is_docker && (
                            <div className="alert alert--info mb-4">
                                <AlertTriangle size={16} />
                                <span>{t("panel_settings.docker_warning")}</span>
                            </div>
                        )}

                        <div className="info-list">
                            <div className="info-list__item info-list__item--editable">
                                <span className="info-list__label">{t("panel_settings.servers_path")}</span>
                                {panelInfo.is_docker ? (
                                    <span className="info-list__value info-list__value--mono">{panelInfo.servers_dir}</span>
                                ) : (
                                    <div className="info-list__input-group">
                                        <input
                                            type="text"
                                            className="form-input form-input--inline"
                                            value={serversDir}
                                            onChange={(e) => setServersDir(e.target.value)}
                                        />
                                        <button
                                            type="button"
                                            className="btn btn--secondary btn--sm"
                                            onClick={() => setShowServersDirPicker(true)}
                                            title={t("panel_settings.browse")}
                                        >
                                            <FolderSearch size={16} />
                                        </button>
                                    </div>
                                )}
                            </div>

                            <div className="info-list__item info-list__item--editable">
                                <span className="info-list__label">{t("panel_settings.backups_path")}</span>
                                {panelInfo.is_docker ? (
                                    <span className="info-list__value info-list__value--mono">{panelInfo.backups_dir}</span>
                                ) : (
                                    <div className="info-list__input-group">
                                        <input
                                            type="text"
                                            className="form-input form-input--inline"
                                            value={backupsDir}
                                            onChange={(e) => setBackupsDir(e.target.value)}
                                        />
                                        <button
                                            type="button"
                                            className="btn btn--secondary btn--sm"
                                            onClick={() => setShowBackupsDirPicker(true)}
                                            title={t("panel_settings.browse")}
                                        >
                                            <FolderSearch size={16} />
                                        </button>
                                    </div>
                                )}
                            </div>
                        </div>
                    </div>

                    {/* Appearance */}
                    <div className="card">
                        <h3 className="settings-section__title">
                            <Palette size={20} />
                            {t("panel_settings.appearance_title")}
                        </h3>

                        <p className="form-hint mb-4">
                            {t("panel_settings.appearance_hint")}
                        </p>

                        <div className="form-group">
                            <label className="form-label">{t("panel_settings.login_color")}</label>
                            <ColorPicker
                                value={loginCustomization.default_color}
                                onChange={(color) => setLoginCustomization(prev => ({ ...prev, default_color: color }))}
                            />
                        </div>

                        <div className="form-group">
                            <label className="form-label">
                                <Image size={16} className="mr-2 v-middle" />
                                {t("panel_settings.login_bg")}
                            </label>
                            <div className="info-list__input-group">
                                <input
                                    type="url"
                                    placeholder="https://example.com/background.jpg"
                                    className="form-input form-input--inline"
                                    value={loginCustomization.background_url}
                                    onChange={(e) => setLoginCustomization(prev => ({ ...prev, background_url: e.target.value }))}
                                />
                                <div className="file-upload-wrapper">
                                    <input
                                        type="file"
                                        id="bg-upload"
                                        accept="image/*"
                                        className="file-input hidden-input"
                                        onChange={handleImageUpload}
                                    />
                                    <label
                                        htmlFor="bg-upload"
                                        className={`btn btn--secondary btn--sm ${isUploadingImage ? "btn--loading" : ""}`}
                                        title={t("panel_settings.upload_image")}
                                    >
                                        {isUploadingImage ? (
                                            <div className="spinner-sm"></div>
                                        ) : (
                                            <Upload size={16} />
                                        )}
                                    </label>
                                </div>
                            </div>
                        </div>

                        {loginCustomization.background_url && (
                            <div className="login-preview">
                                <label className="form-label">{t("panel_settings.preview")}</label>
                                <div
                                    className="login-preview__image"
                                    style={{ backgroundImage: `url(${loginCustomization.background_url})` }}
                                />
                            </div>
                        )}
                    </div>

                    {/* Save Button */}
                    {saveSuccess && (
                        <div className={`alert ${saveMessage.includes("Redémarrez") ? "alert--warning" : "alert--success"}`}>
                            {saveMessage}
                        </div>
                    )}

                    <button className="btn btn--primary btn--lg" onClick={handleSave} disabled={isSaving}>
                        <Save size={18} />
                        {isSaving ? t("common.save") : t("common.save")}
                    </button>
                </div>
            )}

            {/* Users Tab */}
            {activeTab === "users" && (
                <div>
                    <div className="user-list-header">
                        <input
                            type="text"
                            placeholder={t("panel_settings.search_placeholder")}
                            className="form-input search-input"
                            value={searchQuery}
                            onChange={(e) => setSearchQuery(e.target.value)}
                        />
                        <Link to="/panel-settings/users/new" className="btn btn--primary">
                            <Plus size={18} />
                            {t("users.create_user")}
                        </Link>
                    </div>

                    <Table>
                        <thead>
                            <tr>
                                <th>{t("users.username")}</th>
                                <th>{t("users.role")}</th>
                                <th>{t("users.status")}</th>
                                <th>{t("users.last_login")}</th>
                                <th className="table-col-actions">{t("common.actions")}</th>
                            </tr>
                        </thead>
                        <tbody>
                            {filteredUsers.map((user) => (
                                <tr key={user.id} className={!user.is_active ? "table__row--disabled" : ""}>
                                    <td>
                                        <div className="user-cell">
                                            <div className="user-cell__avatar" style={{ backgroundColor: user.accent_color }}>
                                                {user.username.charAt(0).toUpperCase()}
                                            </div>
                                            <div className="user-cell__info">
                                                <span className="user-cell__name">{user.username}</span>
                                                <span className="user-cell__created">{t("common.create")} {formatDate(user.created_at)}</span>
                                            </div>
                                        </div>
                                    </td>
                                    <td>
                                        <span className={`badge badge--${user.role === "admin" ? "primary" : "secondary"}`}>
                                            {user.role === "admin" ? <><Shield size={12} /> Admin</> : <><UserIcon size={12} /> User</>}
                                        </span>
                                    </td>
                                    <td>
                                        <span className={`badge badge--${user.is_active ? "success" : "danger"}`}>
                                            {user.is_active ? t("common.active") : t("common.inactive")}
                                        </span>
                                    </td>
                                    <td>
                                        <div className="last-login-cell">
                                            <div className="text-muted">{formatDate(user.last_login)}</div>
                                            {user.last_ip && <div className="text-xs text-muted-more">{user.last_ip}</div>}
                                        </div>
                                    </td>
                                    <td>
                                        <div className="table__actions">
                                            <Tooltip content={user.is_active ? t("common.disable") : t("common.enable")} position="top">
                                                <button
                                                    className="btn btn--icon btn--ghost"
                                                    onClick={() => handleToggleUserActive(user)}
                                                >
                                                    {user.is_active ? <ShieldOff size={16} /> : <Shield size={16} />}
                                                </button>
                                            </Tooltip>
                                            <Tooltip content={t("common.edit")} position="top">
                                                <Link
                                                    to={`/panel-settings/users/${user.id}`}
                                                    className="btn btn--icon btn--ghost"
                                                >
                                                    <Edit2 size={16} />
                                                </Link>
                                            </Tooltip>
                                            <Tooltip content={t("common.delete")} position="top">
                                                <button
                                                    className="btn btn--icon btn--ghost btn--danger"
                                                    onClick={() => handleDeleteUser(user)}
                                                >
                                                    <Trash2 size={16} />
                                                </button>
                                            </Tooltip>
                                        </div>
                                    </td>
                                </tr>
                            ))}
                        </tbody>
                    </Table>
                </div>
            )}

            {/* Roles Tab */}
            {activeTab === "roles" && (
                <RoleManagement />
            )}

            {/* Directory Pickers */}
            <DirectoryPicker
                isOpen={showServersDirPicker}
                onClose={() => setShowServersDirPicker(false)}
                onSelect={(path) => setServersDir(path)}
                initialPath={serversDir || "/"}
                title={t("panel_settings.servers_dir_title")}
            />

            <DirectoryPicker
                isOpen={showBackupsDirPicker}
                onClose={() => setShowBackupsDirPicker(false)}
                onSelect={(path) => setBackupsDir(path)}
                initialPath={backupsDir || "/"}
                title={t("panel_settings.backups_dir_title")}
            />
        </div>
    );
}
