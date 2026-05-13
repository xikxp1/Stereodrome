import { createContext, useCallback, useContext, useMemo, useRef } from "react";

export type IpodInput =
  | "scroll_backward"
  | "scroll_forward"
  | "select"
  | "select_long"
  | "menu"
  | "menu_long"
  | "play_pause"
  | "previous"
  | "next";

type Listener = (input: IpodInput) => void;

type InputContextValue = {
  emit(input: IpodInput): void;
  subscribe(listener: Listener): () => void;
};

const InputContext = createContext<InputContextValue | null>(null);

export function InputProvider({ children }: { children: React.ReactNode }) {
  const listeners = useRef(new Set<Listener>());

  const emit = useCallback((input: IpodInput) => {
    listeners.current.forEach((listener) => listener(input));
  }, []);

  const subscribe = useCallback((listener: Listener) => {
    listeners.current.add(listener);
    return () => listeners.current.delete(listener);
  }, []);

  const value = useMemo(() => ({ emit, subscribe }), [emit, subscribe]);

  return (
    <InputContext.Provider value={value}>{children}</InputContext.Provider>
  );
}

export function useInputBus() {
  const value = useContext(InputContext);
  if (!value) {
    throw new Error("useInputBus must be used inside InputProvider");
  }
  return value;
}
