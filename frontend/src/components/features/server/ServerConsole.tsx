import React, { memo, useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import SafeAnsi from "@/components/shared/SafeAnsi";
import { ArrowDown, Check, ChevronUp, Clipboard, Clock, Download, Search, Send, Terminal, X } from "lucide-react";
import { gameCommands } from "@/constants/gameCommands";
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

const FILTER_LEVELS = ["error", "warning", "info"] as const;
type FilterLevel = (typeof FILTER_LEVELS)[number];

// Horodatage en tête de ligne, tel que l'émettent Hytale, Minecraft et la
// plupart des serveurs dédiés : « [2026/09/01 08:16:51 INFO] » ou « [08:16:51] ».
const LEADING_TIMESTAMP = /^\[\d{2,4}[/\-:]\d{2}[/\-:]\d{2}[T ]?[\d:.]*\s*/;

function withoutTimestamp(line: string): string {
    const stripped = line.replace(LEADING_TIMESTAMP, "");
    // Une ligne entièrement consommée n'apportait rien : on garde l'originale.
    return stripped.trim().length > 0 ? stripped : line;
}

// Mémoïsé : une ligne inchangée n'est ni reclassée ni re-rendue quand de
// nouvelles lignes arrivent en fin de journal.
const ConsoleLine = memo(function ConsoleLine(
    { line, highlight, showTimestamps }: { line: string; highlight: string; showTimestamps: boolean },
) {
    const level = levelOf(line);
    const text = showTimestamps ? line : withoutTimestamp(line);
    return (
        <div className={level ? `console-line console-line--${level}` : "console-line"}>
            <SafeAnsi highlight={highlight}>{text}</SafeAnsi>
        </div>
    );
});

interface ServerConsoleProps {
    historyKey: string;
    /** Profil du jeu, qui détermine les commandes proposées à la saisie. */
    profileId: string;
    logs: string[];
    isRunning: boolean;
    isInstalling?: boolean;
    onSendCommand: (command: string) => void;
}

const COMMAND_HISTORY_PREFIX = "dmx_console_history:";
const MAX_COMMAND_HISTORY = 100;

// L'historique vivait dans une `Map` de module, perdue au moindre rechargement
// et jamais purgée à mesure que l'on visitait des instances.
function loadCommandHistory(key: string): string[] {
    try {
        const stored = JSON.parse(localStorage.getItem(`${COMMAND_HISTORY_PREFIX}${key}`) ?? "[]");
        return Array.isArray(stored) ? stored.filter((entry): entry is string => typeof entry === "string") : [];
    } catch {
        return [];
    }
}

function saveCommandHistory(key: string, history: string[]): void {
    try {
        localStorage.setItem(`${COMMAND_HISTORY_PREFIX}${key}`, JSON.stringify(history.slice(-MAX_COMMAND_HISTORY)));
    } catch {
        // Navigation privée : l'historique reste valable pour la session.
    }
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
    historyKey,
    profileId,
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
    const [search, setSearch] = useState("");
    const [levels, setLevels] = useState<ReadonlySet<FilterLevel>>(new Set());
    const [showTimestamps, setShowTimestamps] = useState(true);
    // « Collé au bas » : l'état pilote à la fois le suivi automatique et le
    // bouton de retour. Un `ref` double la valeur pour les effets de mise en
    // page, qui s'exécutent avant que le rendu suivant ait lieu.
    const [pinned, setPinned] = useState(true);
    const pinnedRef = useRef(true);
    const frozenStartRef = useRef(0);
    const unreadFromRef = useRef(0);
    const commandHistoryRef = useRef<string[]>([]);
    const commandHistoryIndexRef = useRef<number | null>(null);
    const commandDraftRef = useRef("");

    useEffect(() => {
        commandHistoryRef.current = loadCommandHistory(historyKey);
        commandHistoryIndexRef.current = null;
        commandDraftRef.current = "";
        setCommand("");
    }, [historyKey]);

    // La tolérance couvre l'arrondi du sous-pixel et une ligne partiellement
    // visible : à 5 px, une ligne haute suffisait à décrocher le suivi sans que
    // l'utilisateur ait rien fait.
    const BOTTOM_TOLERANCE = 32;

    const handleScroll = useCallback(() => {
        const element = consoleContentRef.current;
        if (!element) return;
        const atBottom =
            element.scrollHeight - element.scrollTop - element.clientHeight <= BOTTOM_TOLERANCE;
        if (atBottom === pinnedRef.current) return;
        pinnedRef.current = atBottom;
        setPinned(atBottom);
    }, []);

    const scrollToBottom = useCallback(() => {
        const element = consoleContentRef.current;
        if (!element) return;
        pinnedRef.current = true;
        setPinned(true);
        element.scrollTop = element.scrollHeight;
    }, []);

    // Recollage après chaque rendu, sans liste de dépendances : la hauteur ne
    // dépend pas que des lignes reçues, mais aussi des filtres, de l'affichage
    // des horodatages et du retour sur l'onglet. L'ancienne version n'écoutait
    // que `logs` et laissait la console figée à mi-hauteur dans tous les autres
    // cas.
    useLayoutEffect(() => {
        if (!pinnedRef.current) return;
        const element = consoleContentRef.current;
        if (!element) return;
        element.scrollTop = element.scrollHeight;
        // `content-visibility: auto` ne donne qu'une hauteur estimée tant que
        // les lignes entrées dans le viewport ne sont pas rendues : la première
        // passe visait donc un bas de page qui n'était pas encore le bon.
        const frame = requestAnimationFrame(() => {
            const current = consoleContentRef.current;
            if (pinnedRef.current && current) current.scrollTop = current.scrollHeight;
        });
        return () => cancelAnimationFrame(frame);
    });

    const handleSubmit = (e: React.FormEvent) => {
        e.preventDefault();
        const normalized = command.trim();
        if (!normalized) return;
        onSendCommand(normalized);
        const history = commandHistoryRef.current;
        if (history.at(-1) !== normalized) {
            commandHistoryRef.current = [...history, normalized].slice(-MAX_COMMAND_HISTORY);
            saveCommandHistory(historyKey, commandHistoryRef.current);
        }
        commandHistoryIndexRef.current = null;
        commandDraftRef.current = "";
        setCommand("");
    };

    const handleCommandHistory = (event: React.KeyboardEvent<HTMLInputElement>) => {
        // Tabulation : complète depuis les commandes documentées du jeu, puis
        // depuis ce qui a déjà été saisi sur cette instance.
        if (event.key === "Tab" && command.trim().length > 0) {
            // Ce que l'utilisateur a déjà tapé passe devant : c'est sa formulation
            // exacte, arguments compris. Le catalogue prend le relais quand
            // l'historique ne répond pas — au premier usage, notamment, où il
            // était vide et ne proposait donc rien.
            const match = [...commandHistoryRef.current].reverse()
                .concat(suggestions.map(({ command: entry }) => entry))
                .find((entry) => entry.startsWith(command) && entry !== command);
            if (match) {
                event.preventDefault();
                setCommand(match);
            }
            return;
        }
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

    const suggestions = useMemo(() => gameCommands(profileId), [profileId]);
    const suggestionListId = `console-commands-${useId()}`;

    // Le filtrage précède la fenêtre de queue : chercher dans les mille dernières
    // lignes affichées plutôt que dans le journal entier n'aurait servi à rien.
    const matchingLogs = useMemo(() => {
        const needle = search.trim().toLowerCase();
        if (needle.length === 0 && levels.size === 0) return logs;
        return logs.filter((line) => {
            if (needle.length > 0 && !line.toLowerCase().includes(needle)) return false;
            if (levels.size === 0) return true;
            const level = levelOf(line);
            return level !== null && levels.has(level as FilterLevel);
        });
    }, [levels, logs, search]);

    // Tant que la vue est décrochée, le début de la fenêtre reste fixe.
    // Auparavant la fenêtre de queue glissait à chaque ligne reçue : les lignes
    // retirées en haut faisaient remonter tout le contenu sous les yeux du
    // lecteur, qui perdait sa place sans avoir touché à rien.
    const hiddenCount = useMemo(() => {
        const tailStart = showFullHistory ? 0 : Math.max(0, matchingLogs.length - TAIL_WINDOW);
        if (pinned) {
            frozenStartRef.current = tailStart;
            unreadFromRef.current = matchingLogs.length;
            return tailStart;
        }
        // Garde-fou : une installation bavarde lue depuis le haut ferait sinon
        // croître le DOM sans limite. Passé ce seuil, la fenêtre reprend sa
        // course — un saut vaut mieux qu'un onglet qui se fige.
        if (matchingLogs.length - frozenStartRef.current > TAIL_WINDOW * 4) {
            frozenStartRef.current = tailStart;
            return tailStart;
        }
        return Math.min(frozenStartRef.current, tailStart);
    }, [matchingLogs.length, pinned, showFullHistory]);

    const visibleLogs = useMemo(
        () => hiddenCount > 0 ? matchingLogs.slice(hiddenCount) : matchingLogs,
        [hiddenCount, matchingLogs],
    );
    const unreadCount = pinned ? 0 : Math.max(0, matchingLogs.length - unreadFromRef.current);
    const filtering = search.trim().length > 0 || levels.size > 0;

    const toggleLevel = useCallback((level: FilterLevel) => {
        setLevels((current) => {
            const next = new Set(current);
            if (next.has(level)) next.delete(level); else next.add(level);
            return next;
        });
    }, []);

    const downloadLogs = useCallback(() => {
        const body = (filtering ? matchingLogs : logs).join("\n");
        const url = URL.createObjectURL(new Blob([body], { type: "text/plain;charset=utf-8" }));
        const link = document.createElement("a");
        link.href = url;
        link.download = `${historyKey.replaceAll(/[^a-z0-9._-]/gi, "-")}.log`;
        document.body.appendChild(link);
        link.click();
        link.remove();
        URL.revokeObjectURL(url);
    }, [filtering, historyKey, logs, matchingLogs]);

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
                        <Tooltip content={t(showTimestamps ? "server_detail.console.hide_timestamps" : "server_detail.console.show_timestamps")} position="bottom">
                            <Button
                                type="button"
                                variant="ghost"
                                size="icon"
                                aria-label={t(showTimestamps ? "server_detail.console.hide_timestamps" : "server_detail.console.show_timestamps")}
                                aria-pressed={showTimestamps}
                                onClick={() => setShowTimestamps((value) => !value)}
                            >
                                <Clock size={15} />
                            </Button>
                        </Tooltip>
                        <Tooltip content={t("server_detail.console.download_logs")} position="bottom">
                            <Button
                                type="button"
                                variant="ghost"
                                size="icon"
                                aria-label={t("server_detail.console.download_logs")}
                                disabled={logs.length === 0}
                                onClick={downloadLogs}
                            >
                                <Download size={15} />
                            </Button>
                        </Tooltip>
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

                <div className="console-filters">
                    <span className="console-filters__search">
                        <Search size={15} aria-hidden="true" />
                        <input
                            type="search"
                            value={search}
                            onChange={(event) => setSearch(event.target.value)}
                            placeholder={t("server_detail.console.search_placeholder")}
                            aria-label={t("server_detail.console.search_placeholder")}
                        />
                        {search.length > 0 && (
                            <button type="button" aria-label={t("common.clear")} onClick={() => setSearch("")}>
                                <X size={14} aria-hidden="true" />
                            </button>
                        )}
                    </span>
                    <span className="console-filters__levels" role="group" aria-label={t("server_detail.console.filter_levels")}>
                        {FILTER_LEVELS.map((level) => (
                            <button
                                key={level}
                                type="button"
                                className={`console-filters__level console-filters__level--${level} ${levels.has(level) ? "is-active" : ""}`}
                                aria-pressed={levels.has(level)}
                                onClick={() => toggleLevel(level)}
                            >
                                {t(`server_detail.console.levels.${level}`)}
                            </button>
                        ))}
                    </span>
                    {filtering && (
                        <span className="console-filters__count" aria-live="polite">
                            {t("server_detail.console.match_count")
                                .replace("{{shown}}", String(matchingLogs.length))
                                .replace("{{total}}", String(logs.length))}
                        </span>
                    )}
                </div>

                {/* Console Viewport */}
                {/* Conteneur défilant : sans point de tabulation ni rôle, son
                    contenu était inatteignable au clavier (axe scrollable-region-focusable). */}
                <div className="console-viewport">
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
                                <ConsoleLine
                                    key={hiddenCount + index}
                                    line={log}
                                    highlight={search}
                                    showTimestamps={showTimestamps}
                                />
                            ))}
                        </>
                    )}
                </div>

                {/* Retour au flux : visible dès que la vue décroche, avec le
                    nombre de lignes arrivées depuis. Sans lui, rejoindre le bas
                    d'une console bavarde demandait de courser le défilement. */}
                {!pinned && logs.length > 0 && (
                    <button
                        type="button"
                        className="console-viewport__jump"
                        onClick={scrollToBottom}
                    >
                        <ArrowDown size={14} aria-hidden="true" />
                        {unreadCount > 0
                            ? t("server_detail.console.new_lines").replace("{{count}}", String(unreadCount))
                            : t("server_detail.console.jump_to_latest")}
                    </button>
                )}
                </div>

                {/* Command Input Area */}
                <form onSubmit={handleSubmit} className="command-form">
                    <div className="input-wrapper">
                        <span className="prompt-char">{">"}</span>
                        <input
                            type="text"
                            list={suggestions.length > 0 ? suggestionListId : undefined}
                            value={command}
                            onChange={(e) => setCommand(e.target.value)}
                            onKeyDown={handleCommandHistory}
                            placeholder={t("server_detail.console.command_placeholder")}
                            disabled={!isRunning}
                            className="console-input"
                            autoComplete="off"
                        />
                    </div>
                    {/* Suggestions natives : navigables au clavier et annoncées
                        par les lecteurs d'écran sans code d'accessibilité maison. */}
                    {suggestions.length > 0 && (
                        <datalist id={suggestionListId}>
                            {suggestions.map(({ command: entry, hint }) => (
                                <option key={entry} value={entry} label={hint} />
                            ))}
                        </datalist>
                    )}
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
