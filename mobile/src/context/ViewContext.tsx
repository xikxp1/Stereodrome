import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
} from "react";

export type ViewName =
  | "connect"
  | "home"
  | "music"
  | "artists"
  | "artist"
  | "albums"
  | "album"
  | "songs"
  | "playlists"
  | "playlist"
  | "search"
  | "nowPlaying"
  | "settings";

export type ViewInstance = {
  name: ViewName;
  title: string;
  params?: Record<string, string>;
};

type ViewContextValue = {
  stack: ViewInstance[];
  current: ViewInstance;
  push(view: ViewInstance): void;
  pop(): void;
  reset(view: ViewInstance): void;
  showNowPlaying(): void;
};

const ViewContext = createContext<ViewContextValue | null>(null);

const home: ViewInstance = { name: "home", title: "Stereodrome" };

export function ViewProvider({ children }: { children: React.ReactNode }) {
  const [stack, setStack] = useState<ViewInstance[]>([home]);
  const current = stack[stack.length - 1] ?? home;

  const push = useCallback((view: ViewInstance) => {
    setStack((existing) => [...existing, view]);
  }, []);

  const pop = useCallback(() => {
    setStack((existing) =>
      existing.length > 1 ? existing.slice(0, -1) : existing
    );
  }, []);

  const reset = useCallback((view: ViewInstance) => {
    setStack([view]);
  }, []);

  const showNowPlaying = useCallback(() => {
    setStack((existing) => {
      const currentView = existing[existing.length - 1];
      if (currentView?.name === "nowPlaying") {
        return existing;
      }
      return [...existing, { name: "nowPlaying", title: "Now Playing" }];
    });
  }, []);

  const value = useMemo(
    () => ({ stack, current, push, pop, reset, showNowPlaying }),
    [current, pop, push, reset, showNowPlaying, stack]
  );

  return <ViewContext.Provider value={value}>{children}</ViewContext.Provider>;
}

export function useViewStack() {
  const value = useContext(ViewContext);
  if (!value) {
    throw new Error("useViewStack must be used inside ViewProvider");
  }
  return value;
}
