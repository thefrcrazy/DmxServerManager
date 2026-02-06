import React, { useRef, useState, useEffect } from "react";
import { Folder, File, FolderOpen, Save, CornerUpLeft, Home, RefreshCw, FolderPlus, FilePlus, Upload, Trash2, X, Download, CheckSquare, Square, MoreHorizontal, Copy, Move, Pencil } from "lucide-react";
import { formatBytes } from "@/utils/formatters";
import { useLanguage } from "@/contexts/LanguageContext";
import { Tooltip } from "@/components/ui";

interface FileEntry {
    name: string;
    path: string;
    is_dir: boolean;
    size?: number;
    modified_at?: number;
}

interface ServerFilesProps {
    files: FileEntry[];
    currentPath: string;
    isLoading: boolean;
    selectedFile: string | null;
    fileContent: string;
    isSaving: boolean;
    onNavigate: (path: string) => void;
    onReadFile: (path: string) => void;
    onSaveFile: (content: string) => void;
    onCloseEditor: () => void;
    onRefresh: () => void;
    onCreateFolder: (name: string) => void;
    onCreateFile: (name: string) => void;
    onUploadFiles: (files: FileList) => void;
    onDeleteFile: (path: string) => void;
    onRenameFile: (path: string, newName: string) => void;
    onCopyFile: (source: string, destination: string) => void;
    onMoveFile: (source: string, destination: string) => void;
}

type ModalType = "folder" | "file" | "copy" | "move" | "delete" | "delete-multiple" | null;

interface ContextMenuState {
    visible: boolean;
    x: number;
    y: number;
    file: FileEntry | null;
}

export default function ServerFiles({
    files,
    currentPath,
    isLoading,
    selectedFile,
    fileContent,
    isSaving,
    onNavigate,
    onReadFile,
    onSaveFile,
    onCloseEditor,
    onRefresh,
    onCreateFolder,
    onCreateFile,
    onUploadFiles,
    onDeleteFile,
    onRenameFile,
    onCopyFile,
    onMoveFile
}: ServerFilesProps) {
    const { t } = useLanguage();
    const [editorContent, setEditorContent] = React.useState(fileContent);
    const [activeModal, setActiveModal] = useState<ModalType>(null);
    const [inputValue, setInputValue] = useState("");
    const [isDragging, setIsDragging] = useState(false);
    const [selectedItems, setSelectedItems] = useState<Set<string>>(new Set());
    const [contextMenu, setContextMenu] = useState<ContextMenuState>({ visible: false, x: 0, y: 0, file: null });
    const [actionTarget, setActionTarget] = useState<FileEntry | null>(null);
    const [deleteTargets, setDeleteTargets] = useState<string[]>([]);
    const [renamingPath, setRenamingPath] = useState<string | null>(null);
    const [renameValue, setRenameValue] = useState("");
    const fileInputRef = useRef<HTMLInputElement>(null);
    const contextMenuRef = useRef<HTMLDivElement>(null);
    const renameInputRef = useRef<HTMLInputElement>(null);

    // Sync local editor content when fileContent changes (file loaded)
    React.useEffect(() => {
        setEditorContent(fileContent);
    }, [fileContent]);

    // Clear selection when changing directory
    useEffect(() => {
        setSelectedItems(new Set());
    }, [currentPath]);

    // Close context menu on click outside
    useEffect(() => {
        const handleClickOutside = (e: MouseEvent) => {
            if (contextMenuRef.current && !contextMenuRef.current.contains(e.target as Node)) {
                setContextMenu({ visible: false, x: 0, y: 0, file: null });
            }
        };
        document.addEventListener("mousedown", handleClickOutside);
        return () => document.removeEventListener("mousedown", handleClickOutside);
    }, []);

    const pathParts = currentPath.split("/").filter(p => p);

    // Format date
    const formatDate = (timestamp?: number) => {
        if (!timestamp) return "-";
        const date = new Date(timestamp * 1000);
        return date.toLocaleDateString("fr-FR", {
            day: "2-digit",
            month: "2-digit",
            year: "numeric",
            hour: "2-digit",
            minute: "2-digit"
        });
    };

    // Get file type/extension
    const getFileType = (file: FileEntry) => {
        if (file.is_dir) return "Dossier";
        const ext = file.name.split(".").pop()?.toLowerCase() || "";
        const types: Record<string, string> = {
            "jar": "Java Archive",
            "json": "JSON",
            "yml": "YAML",
            "yaml": "YAML",
            "properties": "Properties",
            "txt": "Texte",
            "log": "Log",
            "md": "Markdown",
            "toml": "TOML",
            "cfg": "Config",
            "conf": "Config",
            "sh": "Script Shell",
            "bat": "Script Batch",
            "png": "Image PNG",
            "jpg": "Image JPEG",
            "gif": "Image GIF",
            "zip": "Archive ZIP",
            "gz": "Archive GZ",
        };
        return types[ext] || ext.toUpperCase() || "Fichier";
    };

    const handleModalSubmit = () => {
        if (!inputValue.trim()) return;
        if (activeModal === "folder") {
            onCreateFolder(inputValue.trim());
        } else if (activeModal === "file") {
            onCreateFile(inputValue.trim());
        } else if (activeModal === "copy" && actionTarget) {
            const destPath = currentPath ? `${currentPath}/${inputValue.trim()}` : inputValue.trim();
            onCopyFile(actionTarget.path, destPath);
        } else if (activeModal === "move" && actionTarget) {
            onMoveFile(actionTarget.path, inputValue.trim());
        }
        setInputValue("");
        setActiveModal(null);
        setActionTarget(null);
    };

    const confirmDelete = () => {
        deleteTargets.forEach(path => onDeleteFile(path));
        setDeleteTargets([]);
        setSelectedItems(new Set());
        setActiveModal(null);
        setActionTarget(null);
    };

    const cancelDelete = () => {
        setDeleteTargets([]);
        setActiveModal(null);
        setActionTarget(null);
    };

    // Inline rename functions
    const startRename = (file: FileEntry) => {
        setRenamingPath(file.path);
        setRenameValue(file.name);
        // Focus the input on next tick
        setTimeout(() => renameInputRef.current?.focus(), 10);
    };

    const handleRenameSubmit = () => {
        if (!renamingPath || !renameValue.trim()) {
            cancelRename();
            return;
        }
        onRenameFile(renamingPath, renameValue.trim());
        cancelRename();
    };

    const handleRenameKeyDown = (e: React.KeyboardEvent) => {
        if (e.key === "Enter") {
            e.preventDefault();
            handleRenameSubmit();
        } else if (e.key === "Escape") {
            cancelRename();
        }
    };

    const cancelRename = () => {
        setRenamingPath(null);
        setRenameValue("");
    };

    const handleKeyDown = (e: React.KeyboardEvent) => {
        if (e.key === "Enter") {
            handleModalSubmit();
        } else if (e.key === "Escape") {
            setActiveModal(null);
            setInputValue("");
            setActionTarget(null);
        }
    };

    const handleFileInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
        if (e.target.files && e.target.files.length > 0) {
            onUploadFiles(e.target.files);
            setActiveModal(null);
        }
    };

    const handleDragOver = (e: React.DragEvent) => {
        e.preventDefault();
        setIsDragging(true);
    };

    const handleDragLeave = (e: React.DragEvent) => {
        e.preventDefault();
        setIsDragging(false);
    };

    const handleDrop = (e: React.DragEvent) => {
        e.preventDefault();
        setIsDragging(false);
        if (e.dataTransfer.files && e.dataTransfer.files.length > 0) {
            onUploadFiles(e.dataTransfer.files);
        }
    };

    // Selection handlers
    const toggleSelect = (path: string, e: React.MouseEvent) => {
        e.stopPropagation();
        const newSelected = new Set(selectedItems);
        if (newSelected.has(path)) {
            newSelected.delete(path);
        } else {
            newSelected.add(path);
        }
        setSelectedItems(newSelected);
    };

    const toggleSelectAll = () => {
        if (selectedItems.size === files.filter(f => f.name !== "..").length) {
            setSelectedItems(new Set());
        } else {
            const allPaths = files.filter(f => f.name !== "..").map(f => f.path);
            setSelectedItems(new Set(allPaths));
        }
    };

    const clearSelection = () => {
        setSelectedItems(new Set());
    };

    const deleteSelected = () => {
        if (selectedItems.size === 0) return;
        setDeleteTargets(Array.from(selectedItems));
        setActiveModal("delete-multiple");
    };

    // Context menu
    const handleContextMenu = (e: React.MouseEvent, file: FileEntry) => {
        e.preventDefault();
        e.stopPropagation();
        setContextMenu({
            visible: true,
            x: e.clientX,
            y: e.clientY,
            file
        });
    };

    const handleEmptyContextMenu = (e: React.MouseEvent) => {
        e.preventDefault();
        setContextMenu({
            visible: true,
            x: e.clientX,
            y: e.clientY,
            file: null
        });
    };

    const closeContextMenu = () => {
        setContextMenu({ visible: false, x: 0, y: 0, file: null });
    };

    const handleContextAction = (action: string) => {
        const file = contextMenu.file;
        closeContextMenu();

        // Actions qui ne nécessitent pas de fichier
        switch (action) {
            case "new-folder":
                setActiveModal("folder");
                return;
            case "new-file":
                setActiveModal("file");
                return;
            case "upload":
                fileInputRef.current?.click();
                return;
        }

        // Actions qui nécessitent un fichier
        if (!file) return;

        switch (action) {
            case "open":
                if (file.is_dir) {
                    onNavigate(file.path);
                } else {
                    onReadFile(file.path);
                }
                break;
            case "download":
                if (!file.is_dir) {
                    const serverId = window.location.pathname.split("/")[2];
                    const url = `/api/v1/servers/${serverId}/files/download?path=${encodeURIComponent(file.path)}`;
                    const link = document.createElement("a");
                    link.href = url;
                    link.download = file.name;
                    document.body.appendChild(link);
                    link.click();
                    document.body.removeChild(link);
                }
                break;
            case "rename":
                startRename(file);
                break;
            case "copy":
                setActionTarget(file);
                setInputValue(file.name + "_copy");
                setActiveModal("copy");
                break;
            case "move":
                setActionTarget(file);
                setInputValue(file.path);
                setActiveModal("move");
                break;
            case "delete":
                setActionTarget(file);
                setDeleteTargets([file.path]);
                setActiveModal("delete");
                break;
            case "select": {
                const newSelected = new Set(selectedItems);
                newSelected.add(file.path);
                setSelectedItems(newSelected);
                break;
            }
        }
    };

    // Dropdown menu for options column
    const handleOptionsClick = (e: React.MouseEvent, file: FileEntry) => {
        e.stopPropagation();
        handleContextMenu(e, file);
    };

    const allSelected = files.filter(f => f.name !== "..").length > 0 &&
        selectedItems.size === files.filter(f => f.name !== "..").length;

    return (
        <div className="files-wrapper">
            {/* Integrated Toolbar */}
            <div className="files-toolbar">
                <div className="breadcrumb">
                    <button
                        onClick={() => onNavigate("")}
                        className="breadcrumb-item breadcrumb-root"
                        title="Racine"
                    >
                        <Home size={16} />
                    </button>
                    <span className="breadcrumb-separator">/</span>
                    {pathParts.map((part, index) => (
                        <React.Fragment key={index}>
                            <button
                                onClick={() => onNavigate(pathParts.slice(0, index + 1).join("/"))}
                                className="breadcrumb-item"
                            >
                                {part}
                            </button>
                            <span className="breadcrumb-separator">/</span>
                        </React.Fragment>
                    ))}
                    {currentPath === "" && <span className="breadcrumb-placeholder">{t("server_detail.files.root")}</span>}
                </div>
                <div className="quick-actions">
                    {selectedItems.size > 0 && (
                        <>
                            <span className="selection-count">{selectedItems.size} {t("common.selected") || "sélectionné(s)"}</span>
                            <Tooltip content={t("common.delete")} position="bottom">
                                <button onClick={deleteSelected} className="btn btn--xs btn--danger">
                                    <Trash2 size={14} />
                                </button>
                            </Tooltip>
                            <Tooltip content={t("common.deselect") || "Désélectionner"} position="bottom">
                                <button onClick={clearSelection} className="btn btn--xs btn--ghost">
                                    <X size={14} />
                                </button>
                            </Tooltip>
                            <div className="separator-vertical"></div>
                        </>
                    )}
                    <Tooltip content={t("common.new_folder") || "Nouveau dossier"} position="bottom">
                        <button onClick={() => setActiveModal("folder")} className="btn btn--xs btn--ghost">
                            <FolderPlus size={14} />
                        </button>
                    </Tooltip>
                    <Tooltip content={t("common.new_file") || "Nouveau fichier"} position="bottom">
                        <button onClick={() => setActiveModal("file")} className="btn btn--xs btn--ghost">
                            <FilePlus size={14} />
                        </button>
                    </Tooltip>
                    <Tooltip content={t("common.upload")} position="bottom">
                        <button onClick={() => fileInputRef.current?.click()} className="btn btn--xs btn--ghost">
                            <Upload size={14} />
                        </button>
                    </Tooltip>
                    <input
                        ref={fileInputRef}
                        type="file"
                        multiple
                        style={{ display: "none" }}
                        onChange={handleFileInputChange}
                    />
                    <div className="separator-vertical"></div>
                    <Tooltip content={t("common.refresh")} position="bottom">
                        <button onClick={onRefresh} className="btn btn--xs btn--ghost">
                            <RefreshCw size={14} />
                        </button>
                    </Tooltip>
                    <div className="separator-vertical"></div>
                    <Tooltip content={t("server_detail.tabs.mods")} position="bottom">
                        <button onClick={() => onNavigate("mods")} className="btn btn--xs btn--ghost">
                            {t("server_detail.tabs.mods")}
                        </button>
                    </Tooltip>
                    <Tooltip content="Mondes" position="bottom">
                        <button onClick={() => onNavigate("universe")} className="btn btn--xs btn--ghost">
                            Universe
                        </button>
                    </Tooltip>
                    <Tooltip content={t("server_detail.tabs.logs")} position="bottom">
                        <button onClick={() => onNavigate("logs")} className="btn btn--xs btn--ghost">
                            {t("server_detail.tabs.logs")}
                        </button>
                    </Tooltip>
                </div>
            </div>

            {/* Modal overlay */}
            {activeModal && activeModal !== "delete" && activeModal !== "delete-multiple" && (
                <div className="file-modal-overlay" onClick={() => { setActiveModal(null); setActionTarget(null); }}>
                    <div className="file-modal" onClick={e => e.stopPropagation()}>
                        <div className="file-modal-header">
                            {activeModal === "folder" && <><FolderPlus size={18} /> Nouveau dossier</>}
                            {activeModal === "file" && <><FilePlus size={18} /> Nouveau fichier</>}
                            {activeModal === "copy" && <><Copy size={18} /> Copier</>}
                            {activeModal === "move" && <><Move size={18} /> Déplacer</>}
                            <button className="file-modal-close" onClick={() => { setActiveModal(null); setActionTarget(null); }}>
                                <X size={16} />
                            </button>
                        </div>
                        <div className="file-modal-content">
                            <input
                                type="text"
                                className="form-input"
                                placeholder={
                                    activeModal === "folder" ? "Nom du dossier" :
                                        activeModal === "file" ? "Nom du fichier (ex: config.json)" :
                                            activeModal === "copy" ? "Nom de la copie" :
                                                "Chemin de destination"
                                }
                                value={inputValue}
                                onChange={e => setInputValue(e.target.value)}
                                onKeyDown={handleKeyDown}
                                autoFocus
                            />
                        </div>
                        <div className="file-modal-footer">
                            <button className="btn btn--sm btn--ghost" onClick={() => { setActiveModal(null); setActionTarget(null); }}>
                                Annuler
                            </button>
                            <button className="btn btn--sm btn--primary" onClick={handleModalSubmit} disabled={!inputValue.trim()}>
                                {activeModal === "copy" ? "Copier" : activeModal === "move" ? "Déplacer" : "Créer"}
                            </button>
                        </div>
                    </div>
                </div>
            )}

            {/* Delete Confirmation Modal */}
            {(activeModal === "delete" || activeModal === "delete-multiple") && (
                <div className="file-modal-overlay" onClick={cancelDelete}>
                    <div className="file-modal file-modal--danger" onClick={e => e.stopPropagation()}>
                        <div className="file-modal-header file-modal-header--danger">
                            <Trash2 size={18} />
                            Confirmer la suppression
                            <button className="file-modal-close" onClick={cancelDelete}>
                                <X size={16} />
                            </button>
                        </div>
                        <div className="file-modal-content">
                            <p className="delete-warning">
                                {deleteTargets.length === 1 ? (
                                    <>Êtes-vous sûr de vouloir supprimer <strong>{actionTarget?.name || deleteTargets[0]}</strong> ?</>
                                ) : (
                                    <>Êtes-vous sûr de vouloir supprimer <strong>{deleteTargets.length} éléments</strong> ?</>
                                )}
                            </p>
                            <p className="delete-note">Cette action est irréversible.</p>
                        </div>
                        <div className="file-modal-footer">
                            <button className="btn btn--sm btn--ghost" onClick={cancelDelete}>
                                Annuler
                            </button>
                            <button className="btn btn--sm btn--danger" onClick={confirmDelete}>
                                <Trash2 size={14} />
                                Supprimer
                            </button>
                        </div>
                    </div>
                </div>
            )}

            {/* Context Menu */}
            {contextMenu.visible && (
                <div
                    ref={contextMenuRef}
                    className="context-menu"
                    style={{ top: contextMenu.y, left: contextMenu.x }}
                >
                    {contextMenu.file ? (
                        <>
                            <button className="context-menu-item" onClick={() => handleContextAction("rename")}>
                                <Pencil size={14} />
                                <span>Renommer</span>
                            </button>
                            {!contextMenu.file.is_dir && (
                                <button className="context-menu-item" onClick={() => handleContextAction("download")}>
                                    <Download size={14} />
                                    <span>Télécharger</span>
                                </button>
                            )}
                            <button className="context-menu-item" onClick={() => handleContextAction("copy")}>
                                <Copy size={14} />
                                <span>Copier</span>
                            </button>
                            <button className="context-menu-item" onClick={() => handleContextAction("move")}>
                                <Move size={14} />
                                <span>Déplacer</span>
                            </button>
                            <div className="context-menu-separator"></div>
                            <button className="context-menu-item context-menu-item--danger" onClick={() => handleContextAction("delete")}>
                                <Trash2 size={14} />
                                <span>Supprimer</span>
                            </button>
                        </>
                    ) : (
                        <>
                            <button className="context-menu-item" onClick={() => handleContextAction("new-folder")}>
                                <FolderPlus size={14} />
                                <span>Nouveau dossier</span>
                            </button>
                            <button className="context-menu-item" onClick={() => handleContextAction("new-file")}>
                                <FilePlus size={14} />
                                <span>Nouveau fichier</span>
                            </button>
                            <div className="context-menu-separator"></div>
                            <button className="context-menu-item" onClick={() => handleContextAction("upload")}>
                                <Upload size={14} />
                                <span>Upload</span>
                            </button>
                        </>
                    )}
                </div>
            )}

            {/* Split View */}
            <div
                className={`file-manager ${selectedFile ? "file-manager--with-editor" : ""} ${isDragging ? "file-manager--dragging" : ""}`}
                onDragOver={handleDragOver}
                onDragLeave={handleDragLeave}
                onDrop={handleDrop}
            >

                {/* Drag overlay */}
                {isDragging && (
                    <div className="file-dropzone" style={{ 
                        position: "absolute", 
                        top: 0, 
                        left: 0, 
                        right: 0, 
                        bottom: 0, 
                        background: "rgba(var(--bg-primary-rgb), 0.9)", 
                        zIndex: 100, 
                        display: "flex", 
                        flexDirection: "column", 
                        alignItems: "center", 
                        justifyContent: "center",
                        border: "2px dashed var(--primary-color)",
                        borderRadius: "var(--radius-lg)"
                    }}>
                        <Upload size={64} className="text-primary mb-4 animate-bounce" />
                        <span className="text-xl font-bold text-primary">Déposer les fichiers ici</span>
                        <span className="text-secondary mt-2">Upload immédiat comme un client FTP</span>
                    </div>
                )}

                {/* File List */}
                <div className="file-manager__list-wrapper">
                    <div className="file-tree">
                        <div className="file-tree-header">
                            {t("server_detail.files.explorer")}
                        </div>
                        <div className="file-tree-content" onContextMenu={handleEmptyContextMenu}>
                            {isLoading ? (
                                <div className="loading-container">
                                    <div className="spinner"></div>
                                </div>
                            ) : files.length === 0 ? (
                                <div className="empty-state">
                                    <FolderOpen size={32} />
                                    <span>{t("server_detail.files.empty_folder")}</span>
                                </div>
                            ) : (
                                <div className="file-table-wrapper">
                                    {/* Table Header */}
                                    <div className="file-table-header">
                                        <div className="file-table-cell file-table-cell--checkbox">
                                            <button className="file-item-checkbox" onClick={toggleSelectAll}>
                                                {allSelected ? (
                                                    <CheckSquare size={14} className="checkbox-checked" />
                                                ) : (
                                                    <Square size={14} className="checkbox-unchecked" />
                                                )}
                                            </button>
                                        </div>
                                        <div className="file-table-cell file-table-cell--name">{t("common.name") || "Nom"}</div>
                                        <div className="file-table-cell file-table-cell--type">{t("server_detail.files.file_type")}</div>
                                        <div className="file-table-cell file-table-cell--modified">{t("server_detail.files.modified")}</div>
                                        <div className="file-table-cell file-table-cell--size">{t("server_detail.files.size")}</div>
                                        <div className="file-table-cell file-table-cell--options">{t("server_detail.files.options")}</div>
                                    </div>

                                    {/* Parent directory */}
                                    {currentPath !== "" && (
                                        <div
                                            className="file-table-row file-table-row--parent"
                                            onClick={() => {
                                                const parent = currentPath.split("/").slice(0, -1).join("/");
                                                onNavigate(parent);
                                            }}
                                        >
                                            <div className="file-table-cell file-table-cell--checkbox"></div>
                                            <div className="file-table-cell file-table-cell--name">
                                                <CornerUpLeft size={16} />
                                                <span>..</span>
                                            </div>
                                            <div className="file-table-cell file-table-cell--type">-</div>
                                            <div className="file-table-cell file-table-cell--modified">-</div>
                                            <div className="file-table-cell file-table-cell--size">-</div>
                                            <div className="file-table-cell file-table-cell--options"></div>
                                        </div>
                                    )}

                                    {/* File rows */}
                                    {files.filter(f => f.name !== "..").map((file) => (
                                        <div
                                            key={file.path}
                                            className={`
                                                file-table-row
                                                ${selectedFile === file.path ? "file-table-row--editing" : ""}
                                                ${selectedItems.has(file.path) ? "file-table-row--selected" : ""}
                                            `}
                                            onContextMenu={(e) => handleContextMenu(e, file)}
                                            onClick={() => {
                                                if (file.is_dir) {
                                                    onNavigate(file.path);
                                                } else {
                                                    onReadFile(file.path);
                                                }
                                            }}
                                        >
                                            <div className="file-table-cell file-table-cell--checkbox" onClick={e => e.stopPropagation()}>
                                                <button
                                                    className="file-item-checkbox always-visible"
                                                    onClick={(e) => toggleSelect(file.path, e)}
                                                >
                                                    {selectedItems.has(file.path) ? (
                                                        <CheckSquare size={14} className="checkbox-checked" />
                                                    ) : (
                                                        <Square size={14} className="checkbox-unchecked" />
                                                    )}
                                                </button>
                                            </div>
                                            <div className="file-table-cell file-table-cell--name">
                                                {file.is_dir ? (
                                                    <Folder size={16} className="icon-folder" />
                                                ) : (
                                                    <File size={16} className="icon-file" />
                                                )}
                                                {renamingPath === file.path ? (
                                                    <input
                                                        ref={renameInputRef}
                                                        type="text"
                                                        className="inline-rename-input"
                                                        value={renameValue}
                                                        onChange={(e) => setRenameValue(e.target.value)}
                                                        onKeyDown={handleRenameKeyDown}
                                                        onBlur={handleRenameSubmit}
                                                        onClick={(e) => e.stopPropagation()}
                                                    />
                                                ) : (
                                                    <span className="file-name">{file.name}</span>
                                                )}
                                            </div>
                                            <div className="file-table-cell file-table-cell--type">{getFileType(file)}</div>
                                            <div className="file-table-cell file-table-cell--modified">{formatDate(file.modified_at)}</div>
                                            <div className="file-table-cell file-table-cell--size">
                                                {formatBytes(file.size || 0)}
                                            </div>
                                            <div className="file-table-cell file-table-cell--options" onClick={e => e.stopPropagation()}>
                                                <button
                                                    className="btn-icon"
                                                    onClick={(e) => handleOptionsClick(e, file)}
                                                    title="Options"
                                                >
                                                    <MoreHorizontal size={16} />
                                                </button>
                                            </div>
                                        </div>
                                    ))}
                                </div>
                            )}
                        </div>
                    </div>
                </div>

                {/* File Editor */}
                {selectedFile && (
                    <div className="file-manager__editor-wrapper">
                        <div className="editor-container">
                            {/* Editor Toolbar */}
                            <div className="editor-toolbar">
                                <div className="editor-file-info">
                                    <File size={14} />
                                    <span className="editor-filename">{selectedFile}</span>
                                </div>
                                <div className="editor-actions">
                                    <button
                                        onClick={onCloseEditor}
                                        className="btn btn--xs btn--ghost"
                                    >
                                        {t("server_detail.files.close")}
                                    </button>
                                    <button
                                        onClick={() => onSaveFile(editorContent)}
                                        disabled={isSaving}
                                        className="btn btn--primary btn--sm"
                                    >
                                        <Save size={14} />
                                        {isSaving ? t("common.saving") : t("server_detail.files.save")}
                                    </button>
                                </div>
                            </div>

                            {/* Editor Content */}
                            <textarea
                                value={editorContent}
                                onChange={(e) => setEditorContent(e.target.value)}
                                spellCheck={false}
                                className="editor-textarea"
                            />
                        </div>
                    </div>
                )}
            </div>
        </div>
    );
}
