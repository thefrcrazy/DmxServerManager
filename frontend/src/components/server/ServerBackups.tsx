import React from "react";
import { History, Plus, Download, Trash2, Clock, FileArchive, HardDrive } from "lucide-react";
import { formatBytes } from "../../utils/formatters";

interface Backup {
    id: string;
    server_id: string;
    filename: string;
    size_bytes: number;
    created_at: string;
}

interface ServerBackupsProps {
    backups: Backup[];
    isLoading: boolean;
    isCreating: boolean;
    onCreateBackup: () => void;
    onRestoreBackup: (id: string) => void;
    onDeleteBackup: (id: string) => void;
}

export default function ServerBackups({
    backups,
    isLoading,
    isCreating,
    onCreateBackup,
    onRestoreBackup,
    onDeleteBackup,
}: ServerBackupsProps) {
    return (
        <div className="backups-wrapper">
            {/* Action Header */}
            <div className="section-header">
                <div className="header-info">
                    <h3 className="section-title">
                        <History size={24} />
                        Sauvegardes
                    </h3>
                    <p className="section-subtitle">
                        Gérez les instantanés de votre serveur pour prévenir la perte de données.
                    </p>
                </div>
                <button
                    onClick={onCreateBackup}
                    disabled={isCreating}
                    className="btn btn--primary"
                >
                    {isCreating ? (
                        <>
                            <div className="spinner-sm"></div>
                            Création...
                        </>
                    ) : (
                        <>
                            <Plus size={18} />
                            Nouvelle sauvegarde
                        </>
                    )}
                </button>
            </div>

            {/* List Content */}
            <div className="list-container">
                {isLoading ? (
                    <div className="loading-container">
                        <div className="spinner"></div>
                    </div>
                ) : backups.length === 0 ? (
                    <div className="empty-state">
                        <div className="icon-circle">
                            <History size={32} />
                        </div>
                        <h4>Aucune sauvegarde trouvée</h4>
                        <p>
                            Il semble que vous n'ayez pas encore créé de sauvegarde. C'est fortement recommandé avant de faire des modifications majeures.
                        </p>
                        <button onClick={onCreateBackup} className="btn btn--secondary mt-4">
                            Créer ma première sauvegarde
                        </button>
                    </div>
                ) : (
                    <div className="backup-list">
                        {backups.map((backup) => (
                            <div key={backup.id} className="backup-item">
                                <div className="backup-info">
                                    <div className="backup-icon">
                                        <FileArchive size={20} />
                                    </div>
                                    <div className="backup-details">
                                        <div className="backup-name">
                                            {backup.filename}
                                        </div>
                                        <div className="backup-meta">
                                            <span className="meta-item">
                                                <HardDrive size={12} />
                                                {formatBytes(backup.size_bytes)}
                                            </span>
                                            <span className="meta-separator">•</span>
                                            <span className="meta-item">
                                                <Clock size={12} />
                                                {new Date(backup.created_at).toLocaleString("fr-FR", {
                                                    day: "2-digit",
                                                    month: "short",
                                                    year: "numeric",
                                                    hour: "2-digit",
                                                    minute: "2-digit"
                                                })}
                                            </span>
                                        </div>
                                    </div>
                                </div>
                                <div className="backup-actions">
                                    <button
                                        onClick={() => onRestoreBackup(backup.id)}
                                        title="Restaurer cette sauvegarde"
                                        className="btn btn--icon btn--ghost"
                                    >
                                        <Download size={18} />
                                    </button>
                                    <button
                                        onClick={() => onDeleteBackup(backup.id)}
                                        title="Supprimer définitivement"
                                        className="btn btn--icon btn--ghost btn--danger"
                                    >
                                        <Trash2 size={18} />
                                    </button>
                                </div>
                            </div>
                        ))}
                    </div>
                )}
            </div>
        </div>
    );
}
