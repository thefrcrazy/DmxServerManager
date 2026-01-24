import { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import {
    Plus, Server, FolderOpen, Upload, FolderArchive,
    Rocket, Play
} from 'lucide-react';
import { useLanguage } from '../contexts/LanguageContext';
import { usePageTitle } from '../contexts/PageTitleContext';

type CreationMode = 'normal' | 'existing' | 'zip';



interface ServerFormData {
    name: string;
    game_type: 'hytale';
    executable_path: string;
    working_dir: string;

    bind_address: string;
    port: number;

    auth_mode: 'authenticated' | 'offline';
    allow_op: boolean;

    max_memory: string;
    min_memory: string;
    java_path: string;
    extra_args: string;


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
    const [creationMode, setCreationMode] = useState<CreationMode>('normal');
    const [formData, setFormData] = useState<ServerFormData>({
        // Section 1: Informations générales
        name: '',
        game_type: 'hytale',
        executable_path: 'HytaleServer.jar',
        working_dir: '',

        // Section 2: Configuration Réseau
        bind_address: '0.0.0.0',
        port: 5520,

        // Defaults hidden
        auth_mode: 'authenticated',
        allow_op: false,
        max_memory: '4G',
        min_memory: '4G',
        java_path: '',
        // AOT is hardcoded in backend
        extra_args: '',

        accept_early_plugins: false,
        backup_enabled: false,
        backup_dir: '',
        backup_frequency: 30,
        auto_start: false,
        disable_sentry: false,
        seed: '',
        world_gen_type: 'Hytale',
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
    const [error, setError] = useState('');
    const [isDocker, setIsDocker] = useState(false);
    const [defaultServersDir, setDefaultServersDir] = useState('');

    const { setPageTitle } = usePageTitle();
    useEffect(() => {
        setPageTitle(t('servers.create_new'), 'Configurez votre nouveau serveur Hytale', { to: '/servers' });

        // Fetch settings specifically to check for Docker env and default dir
        fetch('/api/v1/settings', {
            headers: { Authorization: `Bearer ${localStorage.getItem('token')}` }
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
        { id: 'normal' as CreationMode, label: 'Nouveau serveur', icon: Plus, description: 'Créer un nouveau serveur vide' },
        { id: 'existing' as CreationMode, label: 'Serveur existant', icon: FolderOpen, description: 'Importer un serveur déjà configuré' },
        { id: 'zip' as CreationMode, label: 'Importer .zip', icon: FolderArchive, description: 'Importer depuis une archive ZIP' },
    ];

    const handleSubmit = async (e: React.FormEvent) => {
        e.preventDefault();
        setIsSubmitting(true);
        setError('');

        try {
            if (creationMode === 'zip' && zipFile) {
                const formDataUpload = new FormData();
                formDataUpload.append('file', zipFile);
                formDataUpload.append('name', formData.name);
                formDataUpload.append('min_memory', formData.min_memory);
                formDataUpload.append('max_memory', formData.max_memory);
                formDataUpload.append('auto_start', String(formData.auto_start));
                if (formData.java_path) formDataUpload.append('java_path', formData.java_path);
                if (formData.extra_args) formDataUpload.append('extra_args', formData.extra_args);

                const response = await fetch('/api/v1/servers/import-zip', {
                    method: 'POST',
                    headers: {
                        Authorization: `Bearer ${localStorage.getItem('token')}`,
                    },
                    body: formDataUpload,
                });

                if (response.ok) {
                    navigate('/servers');
                } else {
                    const data = await response.json();
                    setError(data.error || 'Erreur lors de l\'importation');
                }
            } else {
                const response = await fetch('/api/v1/servers', {
                    method: 'POST',
                    headers: {
                        'Content-Type': 'application/json',
                        Authorization: `Bearer ${localStorage.getItem('token')}`,
                    },
                    body: JSON.stringify({
                        ...formData,
                        game_type: formData.game_type,
                        java_path: formData.java_path || null,
                        extra_args: formData.extra_args || null,
                        // assets_path: fixed to Assets.zip by backend
                        backup_dir: formData.backup_dir || null,
                        seed: formData.seed || null,
                        import_existing: creationMode === 'existing',
                    }),
                });

                if (response.ok) {
                    const data = await response.json();
                    // Redirect to server detail to see installation logs
                    navigate(`/servers/${data.id}`);
                } else {
                    const data = await response.json();
                    setError(data.error || 'Erreur lors de la création');
                }
            }
        } catch (err) {
            setError('Erreur de connexion au serveur');
            console.error('Erreur:', err);
        } finally {
            setIsSubmitting(false);
        }
    };

    const handleZipChange = (e: React.ChangeEvent<HTMLInputElement>) => {
        const file = e.target.files?.[0];
        if (file && file.name.endsWith('.zip')) {
            setZipFile(file);
            setError('');
        } else {
            setError('Veuillez sélectionner un fichier .zip valide');
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
                        className={`creation-mode-btn ${creationMode === mode.id ? 'creation-mode-btn--active' : ''}`}
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
                        Configuration du serveur
                    </h3>

                    <div className="server-config-grid">

                        {/* Server Name */}
                        <div className="form-group">
                            <label>Nom du serveur</label>
                            <input
                                type="text"
                                value={formData.name}
                                onChange={(e) => updateFormData('name', e.target.value)}
                                placeholder="Mon Serveur"
                                required
                                className="input"
                            />
                        </div>

                        {/* Game Type */}
                        <div className="form-group">
                            <label>Type de jeu</label>
                            <select
                                value={formData.game_type}
                                onChange={(e) => updateFormData('game_type', e.target.value as any)}
                                className="input"
                            >
                                <option value="hytale">Hytale</option>
                            </select>
                        </div>



                        {/* ZIP Upload or Directory */}
                        {creationMode === 'zip' ? (
                            <div className="form-group">
                                <label className="form-label-icon">
                                    <Upload size={14} />
                                    Fichier ZIP
                                </label>
                                <div className={`zip-upload-zone ${zipFile ? 'zip-upload-zone--active' : ''}`}>
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
                                                <p className="zip-upload-text">Cliquez pour sélectionner</p>
                                                <p className="helper-text">
                                                    Archive .zip du serveur
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
                                    {creationMode === 'existing' ? 'Répertoire existant' : 'Répertoire du serveur'}
                                </label>
                                <input
                                    type="text"
                                    value={formData.working_dir}
                                    onChange={(e) => updateFormData('working_dir', e.target.value)}
                                    placeholder="/home/hytale/servers/mon-serveur"
                                    required
                                    className="input font-mono"
                                    readOnly={isDocker}
                                    style={isDocker ? { opacity: 0.7, cursor: 'not-allowed' } : {}}
                                />
                                <p className="helper-text helper-text--block">
                                    {isDocker
                                        ? 'Géré automatiquement par Docker (Volume Persistant)'
                                        : creationMode === 'existing'
                                            ? 'Chemin vers le dossier contenant le serveur existant'
                                            : 'Chemin où le serveur sera installé'}
                                </p>
                            </div>
                        )}

                        <div className="server-config-row">
                            {/* RAM Min */}
                            <div className="form-group">
                                <label>RAM Min (Xms)</label>
                                <input
                                    type="text"
                                    value={formData.min_memory}
                                    onChange={(e) => updateFormData('min_memory', e.target.value)}
                                    placeholder="4G"
                                    className="input"
                                />
                            </div>

                            {/* RAM Max */}
                            <div className="form-group">
                                <label>RAM Max (Xmx)</label>
                                <input
                                    type="text"
                                    value={formData.max_memory}
                                    onChange={(e) => updateFormData('max_memory', e.target.value)}
                                    placeholder="4G"
                                    className="input"
                                />
                            </div>
                        </div>

                        {/* Port UDP */}
                        <div className="form-group">
                            <label>Port UDP</label>
                            <input
                                type="number"
                                value={formData.port}
                                onChange={(e) => updateFormData('port', parseInt(e.target.value) || 5520)}
                                placeholder="5520"
                                className="input"
                            />
                        </div>

                        {/* Hidden Defaults Confirmation */}
                        <div className="advanced-defaults">
                            <div className="advanced-defaults__header">
                                <Rocket size={14} />
                                <span>Paramètres avancés configurés par défaut :</span>
                            </div>
                            <ul>
                                <li>Arguments JVM optimisés (AOT activé)</li>
                                <li>Authentification activée</li>
                                <li>Sauvegardes désactivées (configurable après création)</li>
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
                            onClick={() => navigate('/servers')}
                        >
                            Annuler
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
                                    {creationMode === 'zip' ? <Upload size={18} /> : <Play size={18} />}
                                    {creationMode === 'existing' ? 'Importer le serveur' : 'Créer le serveur'}
                                </>
                            )}
                        </button>
                    </div>
                </div>
            </form>
        </div>
    );
}
