import { useState, useEffect } from 'react';
import { Archive, Download, Trash2, RotateCcw, Box } from 'lucide-react';
import { formatBytes, formatDate } from '../utils/formatters';

interface Backup {
    id: string;
    server_id: string;
    filename: string;
    size_bytes: number;
    created_at: string;
}

import { useLanguage } from '../contexts/LanguageContext';
import { usePageTitle } from '../contexts/PageTitleContext';
import { useToast } from '../contexts/ToastContext';
import { useDialog } from '../contexts/DialogContext';


export default function Backups() {
    const { t } = useLanguage();
    const { success, error } = useToast();
    const { confirm } = useDialog();
    const [backups, setBackups] = useState<Backup[]>([]);
    const [isLoading, setIsLoading] = useState(true);

    useEffect(() => {
        fetchBackups();
    }, []);

    const { setPageTitle } = usePageTitle();
    useEffect(() => {
        setPageTitle(t('backups.title'), t('backups.subtitle'));
    }, [setPageTitle, t]);

    const fetchBackups = async () => {
        try {
            const response = await fetch('/api/v1/backups', {
                headers: { Authorization: `Bearer ${localStorage.getItem('token')}` },
            });
            if (response.ok) {
                setBackups(await response.json());
            }
        } catch (error) {
            console.error('Erreur:', error);
        } finally {
            setIsLoading(false);
        }
    };

    const handleRestore = async (id: string) => {
        if (!await confirm(t('backups.restore_confirm'), { isDestructive: true, confirmLabel: t('backups.restore') })) return;

        try {
            await fetch(`/api/v1/backups/${id}/restore`, {
                method: 'POST',
                headers: { Authorization: `Bearer ${localStorage.getItem('token')}` },
            });
            success(t('backups.restore_launched'));
        } catch {
            error(t('backups.restore_error'));
        }
    };

    const handleDelete = async (id: string) => {
        if (!await confirm(t('backups.delete_confirm'), { isDestructive: true, confirmLabel: t('common.delete') })) return;

        await fetch(`/api/v1/backups/${id}`, {
            method: 'DELETE',
            headers: { Authorization: `Bearer ${localStorage.getItem('token')}` },
        });
        fetchBackups();
    };

    if (isLoading) {
        return (
            <div className="loading-screen">
                <div className="spinner"></div>
                <p className="text-muted">{t('common.loading')}</p>
            </div>
        );
    }

    return (
        <div>

            {backups.length === 0 ? (
                <div className="card empty-state">
                    <div className="empty-state__icon">
                        <Box size={48} />
                    </div>
                    <h3 className="empty-state__title">{t('backups.no_backups')}</h3>
                    <p className="empty-state__description">
                        {t('dashboard.welcome')}
                    </p>
                </div>
            ) : (
                <div className="card">
                    <table className="table">
                        <thead className="table__header">
                            <tr>
                                <th className="table__th">{t('backups.backup_name')}</th>
                                <th className="table__th">{t('backups.size')}</th>
                                <th className="table__th">{t('backups.date')}</th>
                                <th className="table__th table__th--right">{t('backups.actions')}</th>
                            </tr>
                        </thead>
                        <tbody>
                            {backups.map((backup) => (
                                <tr key={backup.id} className="table__row">
                                    <td className="table__cell">
                                        <div className="file-item">
                                            <div className="file-item__icon">
                                                <Archive size={18} />
                                            </div>
                                            <span className="file-item__name">{backup.filename}</span>
                                        </div>
                                    </td>
                                    <td className="table__cell table__cell--mono table__cell--muted">
                                        {formatBytes(backup.size_bytes)}
                                    </td>
                                    <td className="table__cell table__cell--muted">
                                        {formatDate(backup.created_at)}
                                    </td>
                                    <td className="table__cell table__cell--actions">
                                        <button className="btn btn--secondary btn--sm btn--icon" title={t('backups.download')} disabled>
                                            <Download size={16} />
                                        </button>
                                        <button
                                            className="btn btn--secondary btn--sm btn--icon"
                                            onClick={() => handleRestore(backup.id)}
                                            title={t('backups.restore')}
                                        >
                                            <RotateCcw size={16} />
                                        </button>
                                        <button
                                            className="btn btn--danger btn--sm btn--icon"
                                            onClick={() => handleDelete(backup.id)}
                                            title={t('common.delete')}
                                        >
                                            <Trash2 size={16} />
                                        </button>
                                    </td>
                                </tr>
                            ))}
                        </tbody>
                    </table>
                </div>
            )}
        </div>
    );
}
