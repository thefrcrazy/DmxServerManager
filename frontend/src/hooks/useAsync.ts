import { useState, useCallback } from "react";

interface UseAsyncState<T> {
    data: T | null;
    loading: boolean;
    error: string | null;
}

interface UseAsyncReturn<T, Args extends any[]> extends UseAsyncState<T> {
    execute: (...args: Args) => Promise<T | null>;
    reset: () => void;
}

export function useAsync<T, Args extends any[] = []>(
    asyncFn: (...args: Args) => Promise<T>
): UseAsyncReturn<T, Args> {
    const [state, setState] = useState<UseAsyncState<T>>({
        data: null,
        loading: false,
        error: null,
    });

    const execute = useCallback(
        async (...args: Args): Promise<T | null> => {
            setState({ data: null, loading: true, error: null });
            try {
                const result = await asyncFn(...args);
                setState({ data: result, loading: false, error: null });
                return result;
            } catch (err) {
                const message = err instanceof Error ? err.message : "Une erreur est survenue";
                setState({ data: null, loading: false, error: message });
                return null;
            }
        },
        [asyncFn]
    );

    const reset = useCallback(() => {
        setState({ data: null, loading: false, error: null });
    }, []);

    return { ...state, execute, reset };
}
