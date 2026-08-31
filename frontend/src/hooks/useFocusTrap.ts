import { useCallback, useEffect, useRef, type KeyboardEvent, type RefObject } from "react";

const FOCUSABLE_SELECTOR = [
    "button:not([disabled])",
    "a[href]",
    "input:not([disabled])",
    "select:not([disabled])",
    "textarea:not([disabled])",
    "[tabindex]:not([tabindex='-1'])",
].join(", ");

interface UseFocusTrapOptions {
    /** Piège actif. Passer `false` restaure le focus précédent. */
    active?: boolean;
    /** Appelé sur Échap. Omettre pour laisser la touche remonter. */
    onEscape?: () => void;
}

interface UseFocusTrapResult<T extends HTMLElement> {
    containerRef: RefObject<T | null>;
    /** À poser sur le conteneur, qui doit être focalisable (`tabIndex={-1}`). */
    onKeyDown: (event: KeyboardEvent<T>) => void;
}

/**
 * Confine le focus dans une boîte de dialogue et le rend à son origine ensuite.
 *
 * `aria-modal="true"` annonce la modalité aux technologies d'assistance, mais ne
 * rend pas l'arrière-plan inatteignable : sans ce piège, la tabulation sort de
 * la modale et parcourt la page derrière, et le focus retombe sur `<body>` à la
 * fermeture. Extrait de NativeConfigEditorModal, où il était déjà correct.
 */
export function useFocusTrap<T extends HTMLElement>(
    { active = true, onEscape }: UseFocusTrapOptions = {},
): UseFocusTrapResult<T> {
    const containerRef = useRef<T | null>(null);
    const previousFocusRef = useRef<HTMLElement | null>(null);

    useEffect(() => {
        if (!active) return;
        previousFocusRef.current = document.activeElement as HTMLElement | null;
        const frame = requestAnimationFrame(() => containerRef.current?.focus({ preventScroll: true }));
        return () => {
            cancelAnimationFrame(frame);
            previousFocusRef.current?.focus({ preventScroll: true });
        };
    }, [active]);

    const onKeyDown = useCallback((event: KeyboardEvent<T>) => {
        if (event.key === "Escape" && onEscape) {
            event.preventDefault();
            onEscape();
            return;
        }
        if (event.key !== "Tab") return;
        const focusable = [...event.currentTarget.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)]
            .filter((element) => !element.hasAttribute("aria-hidden") && element.offsetParent !== null);
        if (focusable.length === 0) return;
        const first = focusable[0]!;
        const last = focusable.at(-1)!;
        if (event.shiftKey && document.activeElement === first) {
            event.preventDefault();
            last.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
            event.preventDefault();
            first.focus();
        }
    }, [onEscape]);

    return { containerRef, onKeyDown };
}
