import { useEffect, useRef } from "react";
import Ansi from "ansi-to-react";
import { AlertCircle, RefreshCw, Download } from "lucide-react";
import Select from "../../components/Select";
import { enhanceLogContent } from "../../utils/logUtils";

interface FileEntry {
    name: string;
    path: string;
    is_dir: boolean;
}

interface ServerLogsProps {
    logFiles: FileEntry[];
    selectedLogFile: string | null;
    logContent: string;
    serverType?: string;
    onSelectLogFile: (path: string) => void;
    onRefresh: () => void;
}

export default function ServerLogs({
    logFiles,
    selectedLogFile,
    logContent,
    serverType = "hytale",
    onSelectLogFile,
    onRefresh
}: ServerLogsProps) {
    const logsContentRef = useRef<HTMLDivElement>(null);

    // Auto-scroll logic similar to console but for static content rendering
    useEffect(() => {
        if (logContent && logsContentRef.current) {
            logsContentRef.current.scrollTop = logsContentRef.current.scrollHeight;
        }
    }, [logContent, selectedLogFile]);

    const handleDownload = () => {
        if (!selectedLogFile) return;
        const serverId = window.location.pathname.split("/")[2];
        const url = `/api/v1/servers/${serverId}/files/download?path=${encodeURIComponent(selectedLogFile)}`;
        const link = document.createElement("a");
        link.href = url;
        link.download = selectedLogFile.split("/").pop() || "log.txt";
        document.body.appendChild(link);
        link.click();
        document.body.removeChild(link);
    };

    return (
        <div className="logs-wrapper">


            {/* Log Viewer */}
            <div className="console-container logs-container">
                <div className="console-header">
                    <div className="console-header__title">
                        <span className="console-path">{selectedLogFile || "Aucun fichier sélectionné"}</span>
                    </div>

                    <div className="console-header__actions">
                        {logFiles.length > 0 && (
                            <div className="select-wrapper select-wrapper--inline">
                                <Select
                                    options={logFiles.map((f) => ({
                                        label: f.name,
                                        value: f.path,
                                    }))}
                                    value={selectedLogFile || ""}
                                    onChange={(v) => onSelectLogFile(v)}
                                />
                            </div>
                        )}
                        <button
                            onClick={handleDownload}
                            className="btn btn--secondary btn--icon btn--xs"
                            title="Télécharger"
                            disabled={!selectedLogFile}
                        >
                            <Download size={14} />
                        </button>
                        <button onClick={onRefresh} className="btn btn--secondary btn--icon btn--xs" title="Rafraîchir">
                            <RefreshCw size={14} />
                        </button>
                    </div>
                </div>

                <div
                    className="console-output"
                    ref={logsContentRef}
                >
                    {logFiles.length === 0 ? (
                        <div className="console-output__empty">
                            <AlertCircle size={32} />
                            <div className="center-text">
                                <p className="font-medium">Aucun fichier de log trouvé</p>
                                <p className="text-xs">Le dossier logs est vide.</p>
                            </div>
                        </div>
                    ) : (
                        <pre className="log-pre">
                            <Ansi useClasses={false}>
                                {enhanceLogContent(
                                    logContent || "Chargement... ou fichier vide.",
                                    serverType,
                                ) || ""}
                            </Ansi>
                        </pre>
                    )}
                </div>
            </div>
        </div>
    );
}
