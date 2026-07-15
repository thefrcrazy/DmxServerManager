import { KeyboardEvent, useCallback, useEffect, useMemo, useState } from "react";
import { AlertTriangle, RefreshCw, ShieldX } from "lucide-react";
import { CatalogManagement, ModProviderManagement, PanelReleaseManagement, RoleManagement, SteamProfileManagement, UserManagement, WebhookManagement } from "@/components/features/administration";
import { PERMISSION_CATALOG } from "@/constants/permissions";
import { useAuth } from "@/contexts/AuthContext";
import { useLanguage } from "@/contexts/LanguageContext";
import { usePageTitle } from "@/contexts/PageTitleContext";
import { usePermission } from "@/hooks";
import { Instance, ManagedRole, ManagedUser, PermissionDescription } from "@/schemas/api";
import { apiService } from "@/services";
import { translatedError } from "@/utils/roles";

type AdministrationTab = "users" | "roles" | "steam_profiles" | "catalog" | "mod_providers" | "webhooks" | "releases";

export default function Administration() {
    const { user } = useAuth();
    const { t } = useLanguage();
    const { setPageTitle } = usePageTitle();
    const { hasPermission } = usePermission();
    const canRead = hasPermission("user.read");
    const canManageProfiles = hasPermission("profile.manage");
    const [activeTab, setActiveTab] = useState<AdministrationTab>(() => canRead ? "users" : "steam_profiles");
    const [users, setUsers] = useState<ManagedUser[]>([]);
    const [roles, setRoles] = useState<ManagedRole[]>([]);
    const [servers, setServers] = useState<Instance[]>([]);
    const [permissions, setPermissions] = useState<PermissionDescription[]>(PERMISSION_CATALOG);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState("");

    const isOwner = user?.role === "owner";
    const canCreate = hasPermission("user.create");
    const canUpdate = hasPermission("user.update");
    const canReadServers = hasPermission("server.read");

    useEffect(() => {
        setPageTitle(t("administration.title"), t("administration.subtitle"));
    }, [setPageTitle, t]);

    const loadAdministration = useCallback(async () => {
        if (!canRead) {
            setLoading(false);
            return;
        }
        setLoading(true);
        setError("");
        const [rolesResponse, usersResponse, serversResponse, permissionsResponse] = await Promise.all([
            apiService.admin.listRoles(),
            apiService.admin.listUsers(),
            canReadServers ? apiService.servers.getServers() : Promise.resolve(null),
            isOwner ? apiService.admin.listPermissions() : Promise.resolve(null),
        ]);

        const failure = !rolesResponse.success
            ? rolesResponse.error
            : !usersResponse.success
                ? usersResponse.error
                : serversResponse && !serversResponse.success
                    ? serversResponse.error
                    : permissionsResponse && !permissionsResponse.success
                        ? permissionsResponse.error
                        : null;
        if (failure) {
            setError(translatedError(failure, t("administration.load_error"), t));
            setLoading(false);
            return;
        }

        setRoles(rolesResponse.data);
        setUsers(usersResponse.data);
        setServers(serversResponse?.success ? serversResponse.data : []);
        setPermissions(permissionsResponse?.success ? permissionsResponse.data : PERMISSION_CATALOG);
        setLoading(false);
    }, [canRead, canReadServers, isOwner, t]);

    useEffect(() => {
        void loadAdministration();
    }, [loadAdministration]);

    const saveUser = (saved: ManagedUser) => {
        setUsers((current) => [...current.filter((candidate) => candidate.id !== saved.id), saved]
            .sort((left, right) => left.username.localeCompare(right.username)));
    };

    const saveRole = (saved: ManagedRole) => {
        setRoles((current) => [...current.filter((candidate) => candidate.id !== saved.id), saved]
            .sort((left, right) => {
                if (left.is_system !== right.is_system) return left.is_system ? -1 : 1;
                return left.name.localeCompare(right.name);
            }));
        setUsers((current) => current.map((managedUser) => managedUser.role_id === saved.id
            ? { ...managedUser, role_name: saved.name }
            : managedUser));
    };

    const deleteRole = (id: string) => {
        setRoles((current) => current.filter((role) => role.id !== id));
    };

    const tabs = useMemo<AdministrationTab[]>(() => [
        ...(canRead ? ["users" as const] : []),
        ...(isOwner && canRead ? ["roles" as const] : []),
        ...(canManageProfiles ? ["steam_profiles" as const] : []),
        ...(isOwner ? ["catalog" as const] : []),
        ...(isOwner ? ["mod_providers" as const] : []),
        ...(isOwner ? ["webhooks" as const] : []),
        ...(isOwner ? ["releases" as const] : []),
    ], [canManageProfiles, canRead, isOwner]);

    useEffect(() => {
        if (!tabs.includes(activeTab)) setActiveTab(tabs[0] ?? "users");
    }, [activeTab, tabs]);
    const handleTabKeyDown = (event: KeyboardEvent<HTMLButtonElement>, tab: AdministrationTab) => {
        if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
        event.preventDefault();
        const currentIndex = tabs.indexOf(tab);
        const targetIndex = event.key === "Home"
            ? 0
            : event.key === "End"
                ? tabs.length - 1
                : (currentIndex + (event.key === "ArrowRight" ? 1 : -1) + tabs.length) % tabs.length;
        const target = tabs[targetIndex];
        if (!target) return;
        setActiveTab(target);
        document.getElementById(`administration-tab-${target}`)?.focus();
    };

    if (!canRead && !canManageProfiles) {
        return (
            <section className="card administration-state" role="alert">
                <ShieldX size={32} aria-hidden="true" />
                <h2>{t("administration.access_denied")}</h2>
            </section>
        );
    }

    if (loading) {
        return (
            <div className="administration-state" role="status">
                <span className="spinner" aria-hidden="true" />
                <span>{t("common.loading")}</span>
            </div>
        );
    }

    if (error) {
        return (
            <section className="card administration-state" role="alert">
                <AlertTriangle size={32} aria-hidden="true" />
                <p>{error}</p>
                <button type="button" className="btn btn--primary" onClick={() => void loadAdministration()}>
                    <RefreshCw size={17} aria-hidden="true" />
                    {t("administration.retry")}
                </button>
            </section>
        );
    }

    return (
        <div className="administration-page">
            <div className="administration-tabs" role="tablist" aria-label={t("administration.title")}>
                {tabs.map((tab) => (
                    <button
                        key={tab}
                        id={`administration-tab-${tab}`}
                        type="button"
                        role="tab"
                        aria-selected={activeTab === tab}
                        aria-controls={`administration-panel-${tab}`}
                        tabIndex={activeTab === tab ? 0 : -1}
                        className={activeTab === tab ? "administration-tabs__tab administration-tabs__tab--active" : "administration-tabs__tab"}
                        onClick={() => setActiveTab(tab)}
                        onKeyDown={(event) => handleTabKeyDown(event, tab)}
                    >
                        {t(`administration.tabs.${tab}`)}
                    </button>
                ))}
            </div>

            {activeTab === "users" && (
                <div id="administration-panel-users" role="tabpanel" aria-labelledby="administration-tab-users">
                    <UserManagement
                        users={users}
                        roles={roles}
                        servers={servers}
                        permissions={permissions}
                        currentUserId={user?.id ?? ""}
                        currentRole={user?.role ?? ""}
                        canCreate={canCreate}
                        canUpdate={canUpdate}
                        onUserSaved={saveUser}
                    />
                </div>
            )}

            {isOwner && activeTab === "roles" && (
                <div id="administration-panel-roles" role="tabpanel" aria-labelledby="administration-tab-roles">
                    <RoleManagement
                        roles={roles}
                        permissions={permissions}
                        canManage
                        onRoleSaved={saveRole}
                        onRoleDeleted={deleteRole}
                    />
                </div>
            )}

            {canManageProfiles && activeTab === "steam_profiles" && (
                <div id="administration-panel-steam_profiles" role="tabpanel" aria-labelledby="administration-tab-steam_profiles">
                    <SteamProfileManagement />
                </div>
            )}

            {isOwner && activeTab === "webhooks" && (
                <div id="administration-panel-webhooks" role="tabpanel" aria-labelledby="administration-tab-webhooks">
                    <WebhookManagement />
                </div>
            )}

            {isOwner && activeTab === "catalog" && (
                <div id="administration-panel-catalog" role="tabpanel" aria-labelledby="administration-tab-catalog">
                    <CatalogManagement />
                </div>
            )}

            {isOwner && activeTab === "mod_providers" && (
                <div id="administration-panel-mod_providers" role="tabpanel" aria-labelledby="administration-tab-mod_providers">
                    <ModProviderManagement />
                </div>
            )}

            {isOwner && activeTab === "releases" && (
                <div id="administration-panel-releases" role="tabpanel" aria-labelledby="administration-tab-releases">
                    <PanelReleaseManagement />
                </div>
            )}
        </div>
    );
}
