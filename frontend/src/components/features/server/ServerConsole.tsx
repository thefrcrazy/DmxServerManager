import React, { useEffect, useRef } from "react";
import Ansi from "ansi-to-react";
import { Terminal, Send } from "lucide-react";
import { useLanguage } from "@/contexts/LanguageContext";
import { enhanceLogContent } from "@/utils/logUtils";

interface ServerConsoleProps {
    logs: string[];
    isConnected: boolean;
    isRunning: boolean;
    serverType?: string;
    onSendCommand: (command: string) => void;
}

export default function ServerConsole({
    logs,
    isConnected,
    isRunning,
    serverType = "hytale",
    onSendCommand,
}: ServerConsoleProps) {
    const { t } = useLanguage();
    const consoleContentRef = useRef<HTMLDivElement>(null);
    const [command, setCommand] = React.useState("");

    // Auto-scroll logic
    useEffect(() => {
        if (logs.length > 0) {
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
                {/* Console Viewport */}
                <div
                    className="console-output"
                    ref={consoleContentRef}
                >
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
                    <button
                        type="submit"
                        disabled={!isConnected || !isRunning || !command.trim()}
                        className="btn btn--primary btn--icon"
                        title={t("common.send")}
                    >
                        <Send size={16} />
                    </button>
                </form>
            </div>
        </div>
    );
}
