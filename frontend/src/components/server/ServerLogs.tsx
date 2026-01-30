import { useEffect, useRef } from "react";
import Ansi from "ansi-to-react";
import { FileText, AlertCircle, RefreshCw } from "lucide-react";
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

    return (
        <div className="logs-wrapper">
            {/* Toolbar */}
            <div className="logs-toolbar">
                <div className="section-header-simple">
                    <div className="icon-box">
                        <FileText size={16} />
                    </div>
                    <div>
                        <h3 className="section-title-sm">Archives de Logs</h3>
                        <p className="section-subtitle-sm">Consultez l'historique de votre serveur.</p>
                    </div>
                </div>

                <div className="logs-actions">
                    {logFiles.length > 0 && (
                        <div className="select-wrapper">
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
                    <button onClick={onRefresh} className="btn btn--secondary btn--icon" title="Rafraîchir">
                        <RefreshCw size={16} />
                    </button>
                </div>
            </div>

            {/* Log Viewer */}
            <div className="console-container logs-container">
                <div className="console-header">
                    <span className="console-path">{selectedLogFile || "Aucun fichier sélectionné"}</span>
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
                                )}
                            </Ansi>
                        </pre>
                    )}
                </div>
            </div>
        </div>
    );
}
