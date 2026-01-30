import React from "react";
import { Folder, File, ChevronRight, FolderOpen, Save, CornerUpLeft, Home, RefreshCw } from "lucide-react";
import { formatBytes } from "../../utils/formatters";

interface FileEntry {
    name: string;
    path: string;
    is_dir: boolean;
    size?: number;
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
    onRefresh
}: ServerFilesProps) {
    const [editorContent, setEditorContent] = React.useState(fileContent);

    // Sync local editor content when fileContent changes (file loaded)
    React.useEffect(() => {
        setEditorContent(fileContent);
    }, [fileContent]);

    const pathParts = currentPath.split("/").filter(p => p);

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
                    {currentPath === "" && <span className="breadcrumb-placeholder">racine</span>}
                </div>
                <div className="quick-actions">
                    <button onClick={onRefresh} className="btn btn--xs btn--ghost" title="Actualiser">
                        <RefreshCw size={14} />
                    </button>
                    <div className="separator-vertical"></div>
                    <button onClick={() => onNavigate("mods")} className="btn btn--xs btn--ghost" title="Mods">
                        Mods
                    </button>
                    <button onClick={() => onNavigate("universe")} className="btn btn--xs btn--ghost" title="Mondes">
                        Universe
                    </button>
                    <button onClick={() => onNavigate("logs")} className="btn btn--xs btn--ghost" title="Logs">
                        Logs
                    </button>
                </div>
            </div>

            {/* Split View */}
            <div className={`file-manager ${selectedFile ? "file-manager--with-editor" : ""}`}>

                {/* File List */}
                <div className="file-manager__list-wrapper">
                    <div className="file-tree">
                        <div className="file-tree-header">
                            Explorateur
                        </div>
                        <div className="file-tree-content">
                            {isLoading ? (
                                <div className="loading-container">
                                    <div className="spinner"></div>
                                </div>
                            ) : files.length === 0 ? (
                                <div className="empty-state">
                                    <FolderOpen size={32} />
                                    <span>Dossier vide</span>
                                </div>
                            ) : (
                                <div className="file-list">
                                    {currentPath !== "" && (
                                        <button
                                            onClick={() => {
                                                const parent = currentPath.split("/").slice(0, -1).join("/");
                                                onNavigate(parent);
                                            }}
                                            className="file-item file-item--parent"
                                        >
                                            <CornerUpLeft size={16} />
                                            <span>..</span>
                                        </button>
                                    )}
                                    {files.map((file) => (
                                        <div
                                            key={file.path}
                                            onClick={() => {
                                                if (file.is_dir) {
                                                    onNavigate(file.path);
                                                } else {
                                                    onReadFile(file.path);
                                                }
                                            }}
                                            className={`
                                                file-item
                                                ${selectedFile === file.path ? "file-item--selected" : ""}
                                            `}
                                        >
                                            <div className="file-item-info">
                                                {file.is_dir ? (
                                                    <Folder size={16} className={`icon-folder ${selectedFile === file.path ? "active" : ""}`} />
                                                ) : (
                                                    <File size={16} className="icon-file" />
                                                )}
                                                <span className="file-name">{file.name}</span>
                                            </div>
                                            {!file.is_dir && file.size !== undefined && (
                                                <span className="file-size">
                                                    {formatBytes(file.size)}
                                                </span>
                                            )}
                                            {file.is_dir && (
                                                <ChevronRight size={14} className="icon-chevron" />
                                            )}
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
                                        Fermer
                                    </button>
                                    <button
                                        onClick={() => onSaveFile(editorContent)}
                                        disabled={isSaving}
                                        className="btn btn--primary btn--sm"
                                    >
                                        <Save size={14} />
                                        {isSaving ? "Sauvegarde..." : "Enregistrer"}
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
