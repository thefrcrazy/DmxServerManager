import { useState, useEffect, useCallback } from "react";
import { Plus, Edit2, Trash2, Shield, Check, X } from "lucide-react";
import { Table, Tooltip, Checkbox, Input, Button, Modal } from "@/components/ui";
import { useDialog } from "@/contexts/DialogContext";
import { useToast } from "@/contexts/ToastContext";

interface Role {
    id: string;
    name: string;
    permissions: string[];
    is_system: boolean;
    created_at: string;
    updated_at: string;
}

const ALL_PERMISSIONS = [
    { id: "server.view", label: "Voir les serveurs", group: "Serveur" },
    { id: "server.start", label: "Démarrer", group: "Serveur" },
    { id: "server.stop", label: "Arrêter", group: "Serveur" },
    { id: "server.restart", label: "Redémarrer", group: "Serveur" },
    { id: "server.kill", label: "Forcer l'arrêt", group: "Serveur" },
    { id: "server.console.read", label: "Lire la console", group: "Console" },
    { id: "server.console.write", label: "Envoyer des commandes", group: "Console" },
    { id: "server.files.read", label: "Voir les fichiers", group: "Fichiers" },
    { id: "server.files.write", label: "Modifier les fichiers", group: "Fichiers" },
    { id: "server.files.delete", label: "Supprimer des fichiers", group: "Fichiers" },
    { id: "server.backups.view", label: "Voir les sauvegardes", group: "Sauvegardes" },
    { id: "server.backups.create", label: "Créer une sauvegarde", group: "Sauvegardes" },
    { id: "server.backups.restore", label: "Restaurer une sauvegarde", group: "Sauvegardes" },
    { id: "server.schedules.manage", label: "Gérer les tâches planifiées", group: "Tâches" },
    { id: "users.manage", label: "Gérer les utilisateurs", group: "Administration" },
    { id: "roles.manage", label: "Gérer les rôles", group: "Administration" },
    { id: "settings.manage", label: "Gérer les paramètres du panel", group: "Administration" },
];

export default function RoleManagement() {
    const { confirm } = useDialog();
    const { success, error: showError } = useToast();
    const [roles, setRoles] = useState<Role[]>([]);
    const [isLoading, setIsLoading] = useState(true);
    const [isModalOpen, setIsModalOpen] = useState(false);
    const [editingRole, setEditingRole] = useState<Role | null>(null);

    const fetchRoles = useCallback(async () => {
        setIsLoading(true);
        try {
            const response = await fetch("/api/v1/roles", {
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });
            if (response.ok) {
                setRoles(await response.json());
            }
        } catch (error) {
            console.error("Failed to fetch roles", error);
        } finally {
            setIsLoading(false);
        }
    }, []);

    useEffect(() => {
        fetchRoles();
    }, [fetchRoles]);

    const handleCreate = () => {
        setEditingRole(null);
        setIsModalOpen(true);
    };

    const handleEdit = (role: Role) => {
        setEditingRole(role);
        setIsModalOpen(true);
    };

    const handleDelete = async (role: Role) => {
        if (role.is_system) return;
        if (!await confirm(`Supprimer le rôle "${role.name}" ?`, { isDestructive: true })) return;

        try {
            const response = await fetch(`/api/v1/roles/${role.id}`, {
                method: "DELETE",
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });
            if (response.ok) {
                success("Rôle supprimé");
                fetchRoles();
            }
        } catch (error) {
            showError("Erreur lors de la suppression");
        }
    };

    const handleSave = async (data: { name: string; permissions: string[] }) => {
        try {
            const url = editingRole ? `/api/v1/roles/${editingRole.id}` : "/api/v1/roles";
            const method = editingRole ? "PUT" : "POST";

            const response = await fetch(url, {
                method,
                headers: {
                    "Content-Type": "application/json",
                    Authorization: `Bearer ${localStorage.getItem("token")}`,
                },
                body: JSON.stringify(data),
            });

            if (response.ok) {
                success(editingRole ? "Rôle mis à jour" : "Rôle créé");
                setIsModalOpen(false);
                fetchRoles();
            } else {
                const err = await response.json();
                showError(err.error || "Erreur lors de la sauvegarde");
            }
        } catch (error) {
            showError("Erreur de connexion");
        }
    };

    if (isLoading && roles.length === 0) {
        return <div className="p-8 text-center"><div className="spinner mx-auto" /></div>;
    }

    return (
        <div className="role-management">
            <div className="section-header">
                <div className="flex-col">
                    <h2 className="section-title">Gestion des Rôles</h2>
                    <p className="section-desc">Définissez les permissions granulaires pour vos collaborateurs.</p>
                </div>
                <Button onClick={handleCreate}>
                    <Plus size={18} />
                    Nouveau Rôle
                </Button>
            </div>

            <Table>
                <thead>
                    <tr>
                        <th>Nom du rôle</th>
                        <th>Permissions</th>
                        <th className="text-center">Système</th>
                        <th className="table-col-actions">Actions</th>
                    </tr>
                </thead>
                <tbody>
                    {roles.map((role) => (
                        <tr key={role.id}>
                            <td>
                                <div className="role-name-cell">
                                    <Shield size={16} className={role.is_system ? "text-primary" : "text-secondary"} />
                                    <span>{role.name}</span>
                                </div>
                            </td>
                            <td>
                                <div className="permissions-summary">
                                    {role.permissions.includes("*") ? (
                                        <span className="badge badge--primary">Toutes les permissions (*)</span>
                                    ) : (
                                        <span className="text-muted">
                                            {role.permissions.length} permission(s) active(s)
                                        </span>
                                    )}
                                </div>
                            </td>
                            <td className="text-center">
                                {role.is_system ? <Check size={16} className="text-success mx-auto" /> : <X size={16} className="text-muted mx-auto" />}
                            </td>
                            <td>
                                <div className="table__actions">
                                    <Tooltip content="Modifier" position="top">
                                        <Button variant="ghost" size="icon" onClick={() => handleEdit(role)}>
                                            <Edit2 size={16} />
                                        </Button>
                                    </Tooltip>
                                    {!role.is_system && (
                                        <Tooltip content="Supprimer" position="top">
                                            <Button variant="ghost" size="icon" className="text-danger" onClick={() => handleDelete(role)}>
                                                <Trash2 size={16} />
                                            </Button>
                                        </Tooltip>
                                    )}
                                </div>
                            </td>
                        </tr>
                    ))}
                </tbody>
            </Table>

            {isModalOpen && (
                <RoleModal
                    role={editingRole}
                    onClose={() => setIsModalOpen(false)}
                    onSave={handleSave}
                />
            )}
        </div>
    );
}

interface RoleModalProps {
    role: Role | null;
    onClose: () => void;
    onSave: (data: { name: string; permissions: string[] }) => Promise<void>;
}

function RoleModal({ role, onClose, onSave }: RoleModalProps) {
    const [name, setName] = useState(role?.name || "");
    const [selectedPerms, setSelectedPermissions] = useState<string[]>(role?.permissions || []);
    const [isSaving, setIsSaving] = useState(false);

    const togglePermission = (id: string) => {
        if (selectedPerms.includes("*")) return;
        setSelectedPermissions(prev => 
            prev.includes(id) ? prev.filter(p => p !== id) : [...prev, id]
        );
    };

    const toggleAll = () => {
        if (selectedPerms.length === ALL_PERMISSIONS.length) setSelectedPermissions([]);
        else setSelectedPermissions(ALL_PERMISSIONS.map(p => p.id));
    };

    const handleSubmit = async (e: React.FormEvent) => {
        e.preventDefault();
        setIsSaving(true);
        await onSave({ name, permissions: selectedPerms });
        setIsSaving(false);
    };

    const groupedPermissions = ALL_PERMISSIONS.reduce((acc, perm) => {
        if (!acc[perm.group]) acc[perm.group] = [];
        acc[perm.group].push(perm);
        return acc;
    }, {} as Record<string, typeof ALL_PERMISSIONS>);

    const footer = (
        <>
            <Button variant="ghost" onClick={onClose}>Annuler</Button>
            <Button onClick={handleSubmit} disabled={isSaving} isLoading={isSaving}>
                Enregistrer le rôle
            </Button>
        </>
    );

    const title = (
        <div className="flex items-center gap-2">
            <Shield size={20} className="text-primary" />
            <span>{role ? "Modifier le rôle" : "Créer un rôle"}</span>
        </div>
    );

    return (
        <Modal
            isOpen={true}
            onClose={onClose}
            title={title}
            footer={footer}
            size="lg"
        >
            <form onSubmit={handleSubmit} className="role-modal-content">
                <div className="form-group">
                    <label className="form-label">Nom du rôle</label>
                    <Input
                        value={name}
                        onChange={e => setName(e.target.value)}
                        placeholder="ex: Modérateur"
                        required
                        disabled={role?.is_system}
                    />
                </div>

                <div className="permissions-editor">
                    <div className="permissions-header">
                        <label className="form-label">Permissions granulaires</label>
                        <button type="button" className="text-btn" onClick={toggleAll}>
                            {selectedPerms.length === ALL_PERMISSIONS.length ? "Tout décocher" : "Tout cocher"}
                        </button>
                    </div>

                    {role?.id === "admin" ? (
                        <div className="alert alert--info">
                            <Shield size={16} />
                            <span>Ce rôle système possède toutes les permissions (*) et ne peut pas être restreint.</span>
                        </div>
                    ) : (
                        <div className="permissions-groups">
                            {Object.entries(groupedPermissions).map(([group, perms]) => (
                                <div key={group} className="perm-group">
                                    <h4 className="perm-group-title">{group}</h4>
                                    <div className="perm-grid">
                                        {perms.map(perm => (
                                            <Checkbox
                                                key={perm.id}
                                                checked={selectedPerms.includes(perm.id) || selectedPerms.includes("*")}
                                                onChange={() => togglePermission(perm.id)}
                                                disabled={selectedPerms.includes("*")}
                                                label={perm.label}
                                            />
                                        ))}
                                    </div>
                                </div>
                            ))}
                        </div>
                    )}
                </div>
            </form>
        </Modal>
    );
}
