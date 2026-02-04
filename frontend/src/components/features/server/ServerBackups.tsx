import { History, Plus, Download, Trash2, Clock, FileArchive, HardDrive } from "lucide-react";
import { formatBytes } from "@/utils/formatters";
import { useLanguage } from "@/contexts/LanguageContext";

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
    const { t } = useLanguage();

    return (
        <div className="backups-wrapper">
            {/* Action Header */}
            <div className="section-header">
                <div className="header-info">
                    <h3 className="section-title">
                        <History size={24} />
                        {t("backups.title")}
                    </h3>
                    <p className="section-subtitle">
                        {t("backups.subtitle")}
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
                            {t("backups.creating")}
                        </>
                    ) : (
                        <>
                            <Plus size={18} />
                            {t("backups.create_backup")}
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
                        <h4>{t("backups.no_backups")}</h4>
                        <p>
                            {t("backups.empty_desc")}
                        </p>
                        <button onClick={onCreateBackup} className="btn btn--secondary mt-4">
                            {t("backups.create_first")}
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
                                                {new Date(backup.created_at).toLocaleString()}
                                            </span>
                                        </div>
                                    </div>
                                </div>
                                <div className="backup-actions">
                                    <button
                                        onClick={() => onRestoreBackup(backup.id)}
                                        title={t("backups.tooltip_restore")}
                                        className="btn btn--icon btn--ghost"
                                    >
                                        <Download size={18} />
                                    </button>
                                    <button
                                        onClick={() => onDeleteBackup(backup.id)}
                                        title={t("backups.tooltip_delete")}
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
