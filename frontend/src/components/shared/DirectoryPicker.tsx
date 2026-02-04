import { useState, useEffect, useCallback } from "react";
import { Folder, FolderOpen, ChevronRight, X, Check, ArrowUp, Home } from "lucide-react";

interface DirectoryEntry {
    name: string;
    path: string;
    is_dir: boolean;
}

interface DirectoryPickerProps {
    isOpen: boolean;
    onClose: () => void;
    onSelect: (path: string) => void;
    initialPath?: string;
    title?: string;
}

export default function DirectoryPicker({
    isOpen,
    onClose,
    onSelect,
    initialPath = "/",
    title = "Sélectionner un répertoire"
}: DirectoryPickerProps) {
    const [currentPath, setCurrentPath] = useState(initialPath);
    const [entries, setEntries] = useState<DirectoryEntry[]>([]);
    const [isLoading, setIsLoading] = useState(false);
    const [error, setError] = useState("");

    const fetchDirectory = useCallback(async (path: string) => {
        setIsLoading(true);
        setError("");
        try {
            const response = await fetch(`/api/v1/filesystem/list?path=${encodeURIComponent(path)}`, {
                headers: {
                    "Authorization": `Bearer ${localStorage.getItem("token")}`,
                },
            });

            if (response.ok) {
                const data = await response.json();
                setCurrentPath(data.current_path);
                setEntries(data.entries);
            } else {
                const errorData = await response.json();
                setError(errorData.error || "Erreur lors du chargement");
            }
        } catch (err) {
            setError("Erreur de connexion au serveur");
        } finally {
            setIsLoading(false);
        }
    }, []);

    useEffect(() => {
        if (isOpen) {
            fetchDirectory(initialPath);
        }
    }, [isOpen, initialPath, fetchDirectory]);

    const handleNavigate = (path: string) => {
        fetchDirectory(path);
    };

    const handleSelect = () => {
        onSelect(currentPath);
        onClose();
    };

    const handleGoHome = () => {
        fetchDirectory("/");
    };

    const handleGoUp = () => {
        const parentPath = currentPath.split("/").slice(0, -1).join("/") || "/";
        fetchDirectory(parentPath);
    };

    if (!isOpen) return null;

    return (
        <div className="modal-overlay" onClick={onClose}>
            <div className="modal directory-picker-modal" onClick={(e) => e.stopPropagation()}>
                <div className="modal__header">
                    <h2 className="modal__title">
                        <Folder size={20} />
                        {title}
                    </h2>
                    <button className="modal__close" onClick={onClose}>
                        <X size={20} />
                    </button>
                </div>

                <div className="modal__body">
                    {/* Current path display */}
                    <div className="directory-picker__path">
                        <button
                            className="directory-picker__path-btn"
                            onClick={handleGoHome}
                            title="Aller à la racine"
                        >
                            <Home size={16} />
                        </button>
                        <button
                            className="directory-picker__path-btn"
                            onClick={handleGoUp}
                            disabled={currentPath === "/"}
                            title="Remonter d'un niveau"
                        >
                            <ArrowUp size={16} />
                        </button>
                        <span className="directory-picker__path-text">{currentPath}</span>
                    </div>

                    {/* Directory listing */}
                    <div className="directory-picker__list">
                        {isLoading && (
                            <div className="directory-picker__loading">
                                <div className="spinner"></div>
                                <span>Chargement...</span>
                            </div>
                        )}

                        {error && (
                            <div className="directory-picker__error">
                                <p>{error}</p>
                            </div>
                        )}

                        {!isLoading && !error && entries.length === 0 && (
                            <div className="directory-picker__empty">
                                <p>Aucun sous-répertoire</p>
                            </div>
                        )}

                        {!isLoading && !error && entries.map((entry) => (
                            <button
                                key={entry.path}
                                className="directory-picker__item"
                                onClick={() => handleNavigate(entry.path)}
                            >
                                {entry.name === ".." ? (
                                    <ArrowUp size={18} className="directory-picker__item-icon" />
                                ) : (
                                    <FolderOpen size={18} className="directory-picker__item-icon" />
                                )}
                                <span className="directory-picker__item-name">{entry.name}</span>
                                <ChevronRight size={16} className="directory-picker__item-arrow" />
                            </button>
                        ))}
                    </div>
                </div>

                <div className="modal__footer">
                    <button className="btn btn--secondary" onClick={onClose}>
                        <X size={18} />
                        Annuler
                    </button>
                    <button className="btn btn--primary" onClick={handleSelect}>
                        <Check size={18} />
                        Sélectionner ce répertoire
                    </button>
                </div>
            </div>
        </div>
    );
}
