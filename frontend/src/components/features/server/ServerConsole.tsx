import React, { useEffect, useRef } from "react";
import Ansi from "ansi-to-react";
import { Terminal, Send, AlertTriangle, ExternalLink } from "lucide-react";
import { useLanguage } from "@/contexts/LanguageContext";
import { Tooltip } from "@/components/ui";
import { enhanceLogContent } from "@/utils/logUtils";

interface ServerConsoleProps {
    logs: string[];
    isConnected: boolean;
    isRunning: boolean;
    isAuthRequired?: boolean;
    serverType?: string;
    onSendCommand: (command: string) => void;
    onOpenAuth?: () => void;
}

export default function ServerConsole({
    logs,
    isConnected,
    isRunning,
    isAuthRequired = false,
    serverType = "hytale",
    onSendCommand,
    onOpenAuth,
}: ServerConsoleProps) {
    const { t } = useLanguage();
    const consoleContentRef = useRef<HTMLDivElement>(null);
    const [command, setCommand] = React.useState("");
    const isAtBottomRef = useRef(true);

    // Track scroll position
    const handleScroll = () => {
        if (!consoleContentRef.current) return;
        const { scrollTop, scrollHeight, clientHeight } = consoleContentRef.current;
        // Check if user is near bottom (within 50px tolerance)
        isAtBottomRef.current = scrollHeight - scrollTop - clientHeight < 50;
    };

    // Auto-scroll logic
    useEffect(() => {
        if (logs.length > 0 && isAtBottomRef.current) {
            setTimeout(() => {
                if (consoleContentRef.current) {
                    consoleContentRef.current.scrollTop =
                        consoleContentRef.current.scrollHeight;
                }
            }, 50);
        }
    }, [logs.length]);

    const handleSubmit = (e: React.FormEvent) => {
        e.preventDefault();
        if (!command.trim()) return;
        onSendCommand(command);
        setCommand("");
    };

    return (
        <div className="console-wrapper">
            <div className="console-container">
                {/* Console Header */}
                <div className="console-header">
                    <div className="console-header__title">
                        <Terminal size={14} />
                        <span>server@local:~/console</span>
                    </div>

                </div>

                {/* Console Viewport */}
                <div
                    className="console-output"
                    ref={consoleContentRef}
                    onScroll={handleScroll}
                >
                    {isAuthRequired && (
                        <div className="auth-alert-banner">
                            <div className="auth-alert-banner__content">
                                <AlertTriangle size={18} className="text-warning" />
                                <span>{t("installation.auth_required")}</span>
                            </div>
                            <button onClick={onOpenAuth} className="btn btn--warning btn--sm">
                                <ExternalLink size={14} /> {t("installation.action_required")}
                            </button>
                        </div>
                    )}
                    {logs.length === 0 ? (
                        <div className="console-output__empty">
                            <Terminal size={48} />
                            <div className="center-text">
                                <p className="font-medium">
                                    {isRunning
                                        ? t("server_detail.console.waiting_logs")
                                        : t("server_detail.console.server_offline")}
                                </p>
                                {!isRunning && <p className="text-small">{t("server_detail.console.start_server_hint")}</p>}
                            </div>
                        </div>
                    ) : (
                        logs.map((log, i) => {
                            // Auto-translate known Hytale keys logic preserved
                            let displayLog = log;
                            if (log.includes("server.commands.auth.login.device.success")) {
                                displayLog = displayLog.replace(
                                    "server.commands.auth.login.device.success",
                                    t("hytale.server.commands.auth.login.device.success"),
                                );
                            }
                            if (log.includes("server.commands.auth.login.persistence.saved")) {
                                displayLog = displayLog.replace(
                                    /server\.commands\.auth\.login\.persistence\.saved(?:\{.*?\})?/,
                                    t("hytale.server.commands.auth.login.persistence.saved"),
                                );
                            }

                            const isError = log.includes("[ERROR]") || log.includes("ERROR") || log.includes("Exception");
                            const isWarn = log.includes("[WARN]") || log.includes("WARN");
                            const isInfo = log.includes("[INFO]") || log.includes("INFO");
                            const isCommand = log.startsWith(">");

                            return (
                                <div
                                    key={i}
                                    className={`console-line
                                        ${isError ? "console-line--error" : ""}
                                        ${isWarn ? "console-line--warning" : ""}
                                        ${isInfo ? "console-line--info" : ""}
                                        ${isCommand ? "console-line--command" : ""}
                                    `}
                                >
                                    <Ansi useClasses={false}>
                                        {enhanceLogContent(displayLog, serverType)}
                                    </Ansi>
                                </div>
                            );
                        })
                    )}
                </div>

                {/* Command Input Area */}
                <form onSubmit={handleSubmit} className="command-form">
                    <div className="input-wrapper">
                        <span className="prompt-char">{">"}</span>
                        <input
                            type="text"
                            value={command}
                            onChange={(e) => setCommand(e.target.value)}
                            placeholder={t("server_detail.console.command_placeholder")}
                            disabled={!isConnected || !isRunning}
                            className="console-input"
                            autoComplete="off"
                        />
                    </div>
                    <Tooltip content={t("common.send")} position="top">
                        <button
                            type="submit"
                            disabled={!isConnected || !isRunning || !command.trim()}
                            className="btn btn--primary btn--icon"
                        >
                            <Send size={16} />
                        </button>
                    </Tooltip>
                </form>
            </div>

            <style>{`
                .auth-alert-banner {
                    background: rgba(255, 165, 0, 0.1);
                    border: 1px solid rgba(255, 165, 0, 0.3);
                    border-radius: 8px;
                    padding: 12px 16px;
                    margin-bottom: 16px;
                    display: flex;
                    align-items: center;
                    justify-content: space-between;
                    gap: 12px;
                    animation: fadeIn 0.3s ease-out;
                    position: sticky;
                    top: 0;
                    z-index: 10;
                    backdrop-filter: blur(8px);
                }
                
                .auth-alert-banner__content {
                    display: flex;
                    align-items: center;
                    gap: 10px;
                    font-weight: 500;
                    color: var(--text-primary);
                }
                
                .text-warning { color: #ff9800; }
                
                @keyframes fadeIn {
                    from { opacity: 0; transform: translateY(-10px); }
                    to { opacity: 1; transform: translateY(0); }
                }
            `}</style>
        </div>
    );
}
