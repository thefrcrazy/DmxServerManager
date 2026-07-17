import React, { useEffect, useRef } from "react";
import SafeAnsi from "@/components/shared/SafeAnsi";
import { Check, Clipboard, Send, Terminal } from "lucide-react";
import { useLanguage } from "@/contexts/LanguageContext";
import { Tooltip, Button } from "@/components/ui";

interface ServerConsoleProps {
    logs: string[];
    isConnected: boolean;
    isRunning: boolean;
    isInstalling?: boolean;
    onSendCommand: (command: string) => void;
}

async function copyTextToClipboard(value: string): Promise<boolean> {
    if (navigator.clipboard?.writeText) {
        try {
            await navigator.clipboard.writeText(value);
            return true;
        } catch {
            // LAN deployments may not expose the Clipboard API outside HTTPS.
        }
    }

    const activeElement = document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    const textarea = document.createElement("textarea");
    textarea.value = value;
    textarea.readOnly = true;
    textarea.style.position = "fixed";
    textarea.style.opacity = "0";
    textarea.style.pointerEvents = "none";
    document.body.appendChild(textarea);
    textarea.select();
    textarea.setSelectionRange(0, value.length);
    try {
        return document.execCommand("copy");
    } finally {
        textarea.remove();
        activeElement?.focus();
    }
}

export default function ServerConsole({
    logs,
    isConnected,
    isRunning,
    isInstalling = false,
    onSendCommand,
}: ServerConsoleProps) {
    const { t } = useLanguage();
    const consoleContentRef = useRef<HTMLDivElement>(null);
    const [command, setCommand] = React.useState("");
    const [logsCopied, setLogsCopied] = React.useState(false);
    const isAtBottomRef = useRef(true);

    // Track scroll position
    const handleScroll = () => {
        if (!consoleContentRef.current) return;
        const { scrollTop, scrollHeight, clientHeight } = consoleContentRef.current;
        
        // Check if user is at the bottom (with small 5px tolerance for rounding)
        // If they are at the bottom, we enable auto-scroll
        const isAtBottom = scrollHeight - scrollTop - clientHeight < 5;
        isAtBottomRef.current = isAtBottom;
    };

    // Auto-scroll logic
    useEffect(() => {
        if (logs.length > 0 && isAtBottomRef.current && consoleContentRef.current) {
            // Force scroll to bottom without smooth behavior
            consoleContentRef.current.scrollTo({
                top: consoleContentRef.current.scrollHeight,
                behavior: "auto"
            });
        }
    }, [logs]);

    const handleSubmit = (e: React.FormEvent) => {
        e.preventDefault();
        if (!command.trim()) return;
        onSendCommand(command);
        setCommand("");
    };

    const copyLogs = async () => {
        if (logs.length === 0) return;
        if (await copyTextToClipboard(logs.join("\n"))) {
            setLogsCopied(true);
            window.setTimeout(() => setLogsCopied(false), 2_000);
        } else {
            setLogsCopied(false);
        }
    };

    return (
        <div className="console-wrapper">
            <div className="console-container">
                {/* Console Header */}
                <div className="console-header">
                    <div className="console-header__title">
                        <Terminal size={14} />
                        <span>{isInstalling ? "installer@local:~/install" : "server@local:~/console"}</span>
                    </div>
                    <div className="console-header__actions">
                        <Tooltip content={t(logsCopied ? "server_detail.console.logs_copied" : "server_detail.console.copy_logs")} position="left">
                            <Button
                                type="button"
                                variant="ghost"
                                size="icon"
                                aria-label={t(logsCopied ? "server_detail.console.logs_copied" : "server_detail.console.copy_logs")}
                                disabled={logs.length === 0}
                                onClick={() => void copyLogs()}
                            >
                                {logsCopied ? <Check size={15} /> : <Clipboard size={15} />}
                            </Button>
                        </Tooltip>
                    </div>
                </div>

                {/* Console Viewport */}
                <div
                    className="console-output"
                    ref={consoleContentRef}
                    onScroll={handleScroll}
                >
                    {logs.length === 0 ? (
                        <div className="console-output__empty">
                            <Terminal size={48} />
                            <div className="center-text">
                                <p className="font-medium">
                                    {isInstalling
                                        ? t("server_detail.console.installation_running")
                                        : isRunning
                                        ? t("server_detail.console.waiting_logs")
                                        : t("server_detail.console.server_offline")}
                                </p>
                                {isInstalling
                                    ? <p className="text-small">{t("server_detail.console.installation_hint")}</p>
                                    : !isRunning && <p className="text-small">{t("server_detail.console.start_server_hint")}</p>}
                            </div>
                        </div>
                    ) : (
                        logs.map((log, i) => {
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
                                    <SafeAnsi useClasses={false}>
                                        {log}
                                    </SafeAnsi>
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
                        <Button
                            type="submit"
                            variant="primary"
                            size="icon"
                            aria-label={t("common.send")}
                            disabled={!isConnected || !isRunning || !command.trim()}
                        >
                            <Send size={16} />
                        </Button>
                    </Tooltip>
                </form>
            </div>
        </div>
    );
}
