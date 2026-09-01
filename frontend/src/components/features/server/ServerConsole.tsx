import React, { memo, useEffect, useMemo, useRef, useState } from "react";
import SafeAnsi from "@/components/shared/SafeAnsi";
import { Check, ChevronUp, Clipboard, Send, Terminal } from "lucide-react";
import { useLanguage } from "@/contexts/LanguageContext";
import { Tooltip, Button } from "@/components/ui";

// Le DOM est borné : au-delà, une installation qui rejoue 10 000 lignes créait
// autant de nœuds, chacun avec un span par segment ANSI.
const TAIL_WINDOW = 1_500;

// Motifs ancrés et mutuellement exclusifs. La classification précédente reposait
// sur `log.includes("ERROR")`, qui marquait en erreur tout pseudonyme ou chemin
// contenant la sous-chaîne, et pouvait appliquer les quatre styles à la fois.
const LEVEL_PATTERNS: ReadonlyArray<readonly [string, RegExp]> = [
    ["command", /^>/],
    ["error", /\[(ERROR|SEVERE|FATAL)\]|\bException\b/],
    ["warning", /\[WARN(ING)?\]/],
    ["info", /\[INFO\]/],
];

function levelOf(line: string): string | null {
    return LEVEL_PATTERNS.find(([, pattern]) => pattern.test(line))?.[0] ?? null;
}

// Mémoïsé : une ligne inchangée n'est ni reclassée ni re-rendue quand de
// nouvelles lignes arrivent en fin de journal.
const ConsoleLine = memo(function ConsoleLine({ line }: { line: string }) {
    const level = levelOf(line);
    return (
        <div className={level ? `console-line console-line--${level}` : "console-line"}>
            <SafeAnsi>{line}</SafeAnsi>
        </div>
    );
});

interface ServerConsoleProps {
    historyKey: string;
    logs: string[];
    isRunning: boolean;
    isInstalling?: boolean;
    onSendCommand: (command: string) => void;
}

const commandHistories = new Map<string, string[]>();

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
    historyKey,
    logs,
    isRunning,
    isInstalling = false,
    onSendCommand,
}: ServerConsoleProps) {
    const { t } = useLanguage();
    const consoleContentRef = useRef<HTMLDivElement>(null);
    const [command, setCommand] = React.useState("");
    const [logsCopied, setLogsCopied] = React.useState(false);
    const [showFullHistory, setShowFullHistory] = useState(false);
    const isAtBottomRef = useRef(true);
    const commandHistoryRef = useRef<string[]>([]);
    const commandHistoryIndexRef = useRef<number | null>(null);
    const commandDraftRef = useRef("");

    useEffect(() => {
        commandHistoryRef.current = commandHistories.get(historyKey) ?? [];
        commandHistoryIndexRef.current = null;
        commandDraftRef.current = "";
        setCommand("");
    }, [historyKey]);

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
        const normalized = command.trim();
        if (!normalized) return;
        onSendCommand(normalized);
        const history = commandHistoryRef.current;
        if (history.at(-1) !== normalized) {
            commandHistoryRef.current = [...history, normalized].slice(-100);
            commandHistories.set(historyKey, commandHistoryRef.current);
        }
        commandHistoryIndexRef.current = null;
        commandDraftRef.current = "";
        setCommand("");
    };

    const handleCommandHistory = (event: React.KeyboardEvent<HTMLInputElement>) => {
        if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
        const history = commandHistoryRef.current;
        if (history.length === 0) return;
        const currentIndex = commandHistoryIndexRef.current;
        if (event.key === "ArrowUp") {
            event.preventDefault();
            if (currentIndex === null) commandDraftRef.current = command;
            const nextIndex = currentIndex === null
                ? history.length - 1
                : Math.max(0, currentIndex - 1);
            commandHistoryIndexRef.current = nextIndex;
            setCommand(history[nextIndex] ?? "");
            return;
        }
        if (currentIndex === null) return;
        event.preventDefault();
        const nextIndex = currentIndex + 1;
        if (nextIndex >= history.length) {
            commandHistoryIndexRef.current = null;
            setCommand(commandDraftRef.current);
        } else {
            commandHistoryIndexRef.current = nextIndex;
            setCommand(history[nextIndex] ?? "");
        }
    };

    const hiddenCount = showFullHistory ? 0 : Math.max(0, logs.length - TAIL_WINDOW);
    const visibleLogs = useMemo(
        () => hiddenCount > 0 ? logs.slice(hiddenCount) : logs,
        [hiddenCount, logs],
    );

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
                {/* Conteneur défilant : sans point de tabulation ni rôle, son
                    contenu était inatteignable au clavier (axe scrollable-region-focusable). */}
                <div
                    className="console-output"
                    ref={consoleContentRef}
                    onScroll={handleScroll}
                    tabIndex={0}
                    role="log"
                    aria-label={isInstalling
                        ? t("server_detail.console.installation_running")
                        : t("server_detail.tabs.terminal")}
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
                        <>
                            {hiddenCount > 0 && (
                                <button type="button" className="console-output__earlier" onClick={() => setShowFullHistory(true)}>
                                    <ChevronUp size={14} aria-hidden="true" />
                                    {t("server_detail.console.show_earlier").replace("{{count}}", String(hiddenCount))}
                                </button>
                            )}
                            {visibleLogs.map((log, index) => (
                                <ConsoleLine key={hiddenCount + index} line={log} />
                            ))}
                        </>
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
                            onKeyDown={handleCommandHistory}
                            placeholder={t("server_detail.console.command_placeholder")}
                            disabled={!isRunning}
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
                            disabled={!isRunning || !command.trim()}
                        >
                            <Send size={16} />
                        </Button>
                    </Tooltip>
                </form>
            </div>
        </div>
    );
}
