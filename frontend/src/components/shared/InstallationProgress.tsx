import React, { useState, useEffect } from "react";
import { Terminal, Download, Folder, Check, AlertTriangle, ExternalLink } from "lucide-react";
import { useLanguage } from "@/contexts/LanguageContext";
import Ansi from "ansi-to-react";

interface InstallationProgressProps {
    logs: string[];
    onClose: () => void;
    isInstalling: boolean;
    isAuthRequired?: boolean;
    onSendAuth?: () => void;
}

const stripAnsi = (str: string) => str.replace(/[\u001b\u009b][[()#;?]*(?:[0-9]{1,4}(?:;[0-9]{0,4})*)?[0-9A-ORZcf-nqry=><]/g, "");

const InstallationProgress: React.FC<InstallationProgressProps> = ({ logs, onClose, isInstalling, isAuthRequired, onSendAuth }) => {
    const { t } = useLanguage();

    // Determine current step based on logs
    const steps = [
        { id: "init", label: t("installation.steps.init"), icon: <Terminal size={18} /> },
        { id: "download", label: t("installation.steps.download"), icon: <Download size={18} /> },
        { id: "extract", label: t("installation.steps.extract"), icon: <Folder size={18} /> },
        { id: "finish", label: t("installation.steps.finish"), icon: <Check size={18} /> },
    ];

    const [currentStep, setCurrentStep] = useState(0);
    const [isMinimized, setIsMinimized] = useState(false);
    const [authUrl, setAuthUrl] = useState<string | null>(null);
    const [authCode, setAuthCode] = useState<string | null>(null);
    const [downloadProgress, setDownloadProgress] = useState<{ percent: number; details: string } | null>(null);
    const logsContainerRef = React.useRef<HTMLDivElement>(null);

    // Auto-scroll logs
    useEffect(() => {
        if (logsContainerRef.current) {
            logsContainerRef.current.scrollTop = logsContainerRef.current.scrollHeight;
        }
    }, [logs]);

    useEffect(() => {
        // Determine max step from ALL logs
        let maxStep = 0;
        for (const log of logs) {
            const clean = stripAnsi(log);
            if (clean.includes("Initialization de l'installation") || clean.includes("Starting Hytale Server Installation")) maxStep = Math.max(maxStep, 0);
            if (clean.includes("Téléchargement")) maxStep = Math.max(maxStep, 1);
            if (clean.includes("Extraction") || clean.includes("Décompression")) maxStep = Math.max(maxStep, 2);
            if (clean.includes("Installation terminée") || clean.includes("Installation finished")) maxStep = Math.max(maxStep, 3);
        }
        setCurrentStep(maxStep);

        // Check for Auth URL in ALL logs
        // We join them but also strip ANSI to ensure clean matching
        const fullLogClean = stripAnsi(logs.join("\n"));

        // Pattern: https://oauth.accounts.hytale.com/...
        // The logs provide two URLs usually:
        // 1. "Visit: https://.../verify"
        // 2. "Or visit: https://.../verify?user_code=..."
        // We want to prioritize the second one if it exists.

        // Find ALL matches
        const allLinkMatches = fullLogClean.match(/(https:\/\/(?:oauth\.)?accounts\.hytale\.com\/[^\s\u001b]+)/g);

        let bestUrl: string | null = null;
        if (allLinkMatches && allLinkMatches.length > 0) {
            // Prefer the one with user_code
            const urlWithCode = allLinkMatches.find(u => u.includes("user_code="));
            bestUrl = urlWithCode || allLinkMatches[0];

            // Clean punctuation
            bestUrl = bestUrl.replace(/[).\]]+$/, "");
        }

        // Check for progress bar in recent logs (look at last few lines)
        let foundProgress = false;
        // Search from end to find most recent progress
        for (let i = logs.length - 1; i >= Math.max(0, logs.length - 10); i--) {
            const line = stripAnsi(logs[i]);
            const progressMatch = line.match(/\[=*[\s=]*\]\s*([\d\.]+)%\s*\(([^)]+)\)/);
            if (progressMatch) {
                setDownloadProgress({
                    percent: parseFloat(progressMatch[1]),
                    details: progressMatch[2]
                });
                foundProgress = true;
                break;
            }
        }

        // If we found progress, it implies Auth is passed.
        if (foundProgress) {
            setAuthUrl(null);
            setAuthCode(null);
        } else if (bestUrl) {
            setAuthUrl(bestUrl);

            // Try to extract code from URL if present (user_code=...)
            const codeMatch = bestUrl.match(/user_code=([^&]+)/);
            if (codeMatch) {
                setAuthCode(codeMatch[1]);
            } else {
                // Fallback: Check for "Enter code: XXXXXX" or "Authorization code: XXXXXX"
                // Log example: "Enter code: RJNt7CLJ"
                const manualCodeMatch = fullLogClean.match(/(?:Authorization|Enter) (?:the )?code:\s*([^\s\.]+)/i);
                if (manualCodeMatch) {
                    const code = manualCodeMatch[1].trim();
                    setAuthCode(code);
                    // Construct full URL if missing
                    if (!bestUrl.includes("user_code=")) {
                        const separator = bestUrl.includes("?") ? "&" : "?";
                        setAuthUrl(`${bestUrl}${separator}user_code=${code}`);
                    }
                }
            }
        } else {
            // If no linkMatch and no progress, ensure auth states are cleared
            setAuthUrl(null);
            setAuthCode(null);
        }

    }, [logs]);

    // If not installing and no logs, don't show (sanity check)
    if (!isInstalling && !isAuthRequired && logs.length === 0) return null;

    const isRuntimeAuth = !isInstalling && isAuthRequired;

    if (isMinimized) {
        return (
            <div className="installation-overlay minimized" onClick={() => setIsMinimized(false)}>
                <div className="minimized-badge">
                    {isRuntimeAuth ? <AlertTriangle size={16} className="text-yellow-400" /> : <div className="spinner spinner--sm"></div>}
                    <span>{isRuntimeAuth ? t("installation.auth_required") : t("installation.installing")}</span>
                </div>
            </div>
        );
    }

    return (
        <div className="installation-overlay">
            <div className="installation-overlay__card">
                <div className="installation-header">
                    <div className="flex-col">
                        <h2 className="installation-header__title">
                            {isRuntimeAuth ? t("installation.auth_required") : t("installation.installing")}
                        </h2>
                        <p className="installation-header__subtitle">
                            {isRuntimeAuth ? t("installation.auth_required_sub") : t("installation.installing_sub")}
                        </p>
                    </div>
                    <button onClick={() => setIsMinimized(true)} className="btn btn--icon btn--ghost" title="Minimiser">
                        <div className="w-4 h-1 bg-current rounded"></div>
                    </button>
                </div>

                {!isRuntimeAuth && (
                    <div className="installation-steps">
                        {steps.map((step, index) => {
                            const isComplete = currentStep > index;
                            const isCurrent = currentStep === index;

                            let modifierClass = "";
                            if (isCurrent) modifierClass = "installation-step--current";
                            else if (isComplete) modifierClass = "installation-step--complete";

                            return (
                                <div key={step.id} className={`installation-step ${modifierClass}`}>
                                    <div className="installation-step__icon">
                                        {isComplete ? <Check size={14} /> : step.icon}
                                    </div>
                                    <div className="installation-step__content">
                                        <div className="installation-step__label">{step.label}</div>
                                        {isCurrent && (
                                            <div className="installation-step__loader">
                                                <div className="installation-step__loader-bar"></div>
                                            </div>
                                        )}
                                    </div>
                                </div>
                            );
                        })}
                    </div>
                )}


                {/* Auth Action Required Bubble */}
                {(currentStep < 3 || isRuntimeAuth) && !downloadProgress && (
                    <div className="installation-auth">
                        <div className="installation-auth__title">
                            <AlertTriangle size={18} /> {t("installation.action_required")}
                        </div>
                        <div className="installation-auth__content">
                            {authUrl ? (
                                <>
                                    <p className="text-sm text-yellow-100/80 mb-1">{t("installation.auth_needed")}</p>
                                    <a href={authUrl} target="_blank" rel="noopener noreferrer" className="installation-auth__link">
                                        {authUrl} <ExternalLink size={12} style={{ display: "inline", verticalAlign: "middle" }} />
                                    </a>
                                    {authCode && (
                                        <div>
                                            <span className="text-xs text-muted block mt-2">{t("installation.verification_code")}</span>
                                            <span className="installation-auth__code">{authCode}</span>
                                        </div>
                                    )}
                                </>
                            ) : (
                                <>
                                    <p className="text-sm text-yellow-100/80 mb-3">
                                        {t("installation.waiting_command")}
                                    </p>
                                    <button
                                        onClick={onSendAuth}
                                        className="btn btn--primary btn--sm w-full justification-center"
                                        disabled={!onSendAuth}
                                    >
                                        <Terminal size={14} /> {t("installation.send_auth")}
                                    </button>
                                </>
                            )}
                        </div>
                    </div>
                )}

                {/* Real-time Progress Bar */}
                {downloadProgress && currentStep < 3 && !isRuntimeAuth && (
                    <div className="installation-download">
                        <div className="installation-download__details">
                            <span>{t("installation.downloading_files")}</span>
                            <span className="installation-download__percent">{downloadProgress.percent}%</span>
                        </div>
                        <div className="installation-download__bar-container">
                            <div
                                className="installation-download__bar-fill"
                                style={{ width: `${downloadProgress.percent}%` }}
                            ></div>
                        </div>
                        <div className="installation-download__details" style={{ justifyContent: "flex-end", opacity: 0.7 }}>
                            {downloadProgress.details}
                        </div>
                    </div>
                )}

                {/* Fallback Log Status (Show if no progress bar valid or if Auth needed) */}
                {logs.length > 0 && (currentStep < 3 || isRuntimeAuth) && !downloadProgress && !authUrl && (
                    <div className="installation-status">
                        <span className="installation-status__prefix">&gt;</span>
                        {(() => {
                            // Get last non-empty log, ignoring simple newlines
                            const lastLines = logs.filter(l => l.trim().length > 0);
                            const lastLine = lastLines[lastLines.length - 1] || "...";
                            const clean = stripAnsi(lastLine).replace(/^\[ERR\]\s*/, "");
                            return clean.substring(0, 80) + (clean.length > 80 ? "..." : "");
                        })()}
                    </div>
                )}

                {/* Detailed Logs Collapsible - Always shown to user request */}
                <details className="installation-details">
                    <summary className="installation-details__summary">
                        <Terminal size={12} className="mr-2" /> {t("installation.view_details")} ({logs.length})
                    </summary>
                    <div className="installation-details__content" ref={logsContainerRef}>
                        {logs.length === 0 ? (
                            <div className="text-muted italic opacity-50">{t("installation.waiting_logs")}</div>
                        ) : (
                            logs.map((log, i) => (
                                <div key={i} className="log-line">
                                    <Ansi>{log}</Ansi>
                                </div>
                            ))
                        )}
                    </div>
                </details>
                <div className="installation-actions">
                    {currentStep === 3 && !isRuntimeAuth ? (
                        <button onClick={onClose} className="btn-finish">
                            {t("installation.finish")}
                        </button>
                    ) : (
                        <button onClick={onClose} className="btn-cancel">
                            {isRuntimeAuth ? t("installation.close") : t("installation.cancel_close")}
                        </button>
                    )}
                </div>
            </div>
        </div>
    );
};

export default InstallationProgress;
